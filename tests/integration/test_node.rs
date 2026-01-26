//! TestNode - A single in-process pqcoin node for testing.

use pqcoin::COINBASE_MATURITY;
use pqcoin::blockchain::{
    Address, Block, Blockchain, OutPoint, Serialize, Transaction, TxInput, TxOutput, Witness,
};
use pqcoin::crypto::{self, Hash, PublicKey, SecretKey, ml_dsa_87};
use pqcoin::mempool::Mempool;
use pqcoin::miner::{MineResult, mine_block};
use pqcoin::network::{
    NetworkConfig, NetworkError, NetworkEvent, NetworkService, NetworkState, PeerCommand, SyncState,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tempfile::TempDir;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

/// Storage directory wrapper that can be either temporary or persistent.
pub enum StorageDir {
    /// Temporary directory that gets deleted on drop.
    Temp(TempDir),
    /// Persistent path that survives restarts.
    Persistent(std::path::PathBuf),
}

impl StorageDir {
    fn path(&self) -> &std::path::Path {
        match self {
            StorageDir::Temp(t) => t.path(),
            StorageDir::Persistent(p) => p,
        }
    }
}

/// A test node wrapping a single in-process pqcoin node.
pub struct TestNode {
    /// Node identifier for debugging.
    pub id: usize,
    /// Reference to the blockchain.
    pub blockchain: Arc<RwLock<Blockchain>>,
    /// Reference to the mempool.
    pub mempool: Arc<RwLock<Mempool>>,
    /// Reference to the network state (for peer count queries).
    network_state: Arc<RwLock<NetworkState>>,
    /// The actual listen address (after binding).
    pub listen_addr: SocketAddr,
    /// Event receiver for network events.
    event_rx: mpsc::Receiver<NetworkEvent>,
    /// Channel for submitting mined blocks.
    block_submitter: mpsc::Sender<Block>,
    /// Shutdown signal sender.
    shutdown_tx: Option<oneshot::Sender<()>>,
    /// Handle to the network service task.
    service_handle: Option<JoinHandle<Result<(), NetworkError>>>,
    /// Storage directory (kept alive for the node's lifetime).
    storage_dir: StorageDir,
    /// Miner address for this node.
    pub miner_address: Address,
    /// Keypair for signing transactions.
    pub keypair: (PublicKey, SecretKey),
}

impl TestNode {
    /// Create a test node with a specific port.
    ///
    /// Uses persistent LMDB storage like production to ensure tests exercise
    /// the same code paths as the real node.
    pub async fn create_with_port(
        id: usize,
        port: u16,
        seed_peers: Vec<SocketAddr>,
        genesis: Block,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create temporary storage directory (persists for node lifetime)
        let storage_dir = StorageDir::Temp(TempDir::new()?);

        // Generate keypair for this node
        let keypair = ml_dsa_87::keygen();
        let miner_address = Address::from_public_key(&keypair.0);

        Self::create_with_storage(
            id,
            port,
            seed_peers,
            genesis,
            storage_dir,
            keypair,
            miner_address,
        )
        .await
    }

    /// Create a test node with a specific storage directory.
    ///
    /// This allows restarting a node from existing persistent storage.
    pub async fn create_with_storage(
        id: usize,
        port: u16,
        seed_peers: Vec<SocketAddr>,
        genesis: Block,
        storage_dir: StorageDir,
        keypair: (PublicKey, SecretKey),
        miner_address: Address,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create blockchain with persistent storage (like production)
        let blockchain = Blockchain::open(
            storage_dir.path(),
            genesis,
            2016,       // difficulty_adjustment_interval
            600,        // target_block_time
            50_000_000, // initial_reward
            210_000,    // halving_interval
        )?;
        let blockchain = Arc::new(RwLock::new(blockchain));

        let listen_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        // Configure network
        let config = NetworkConfig {
            listen_addr,
            max_peers: 50,
            max_outbound: 25,
            seed_peers,
        };

        // Create network service
        let mut service = NetworkService::new(blockchain.clone(), config);

        // Take receivers and shared state before starting
        let event_rx = service
            .take_event_receiver()
            .expect("event receiver already taken");
        let block_submitter = service.block_submitter();
        let mempool = service.mempool();
        let network_state = service.state();

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Start the service
        let service_handle =
            tokio::spawn(async move { service.run_with_shutdown(shutdown_rx).await });

        // Wait for service to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        Ok(Self {
            id,
            blockchain,
            mempool,
            network_state,
            listen_addr,
            event_rx,
            block_submitter,
            shutdown_tx: Some(shutdown_tx),
            service_handle: Some(service_handle),
            storage_dir,
            miner_address,
            keypair,
        })
    }

    /// Get the listen address.
    pub fn addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// Get the current blockchain height.
    pub async fn height(&self) -> u64 {
        self.blockchain.read().await.height()
    }

    /// Get the tip hash.
    pub async fn tip_hash(&self) -> Hash {
        self.blockchain.read().await.tip_hash()
    }

    /// Get the current number of connected peers.
    pub async fn peer_count(&self) -> usize {
        self.network_state.read().await.peer_count()
    }

    /// Check if the blockchain has a specific block.
    pub async fn has_block(&self, hash: Hash) -> bool {
        self.blockchain.read().await.has_block(&hash)
    }

    /// Check if the mempool has a specific transaction.
    pub async fn has_tx_in_mempool(&self, txid: Hash) -> bool {
        self.mempool.read().await.contains(&txid)
    }

    /// Wait for the next network event with a timeout.
    pub async fn next_event(&mut self, timeout_duration: Duration) -> Option<NetworkEvent> {
        timeout(timeout_duration, self.event_rx.recv())
            .await
            .ok()
            .flatten()
    }

    /// Drain all pending events.
    pub fn drain_events(&mut self) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }
        events
    }

    /// Wait for an event matching the predicate.
    ///
    /// Returns the matching event if found within the timeout, None otherwise.
    pub async fn wait_for_event<P>(
        &mut self,
        predicate: P,
        timeout_duration: Duration,
    ) -> Option<NetworkEvent>
    where
        P: Fn(&NetworkEvent) -> bool,
    {
        let deadline = tokio::time::Instant::now() + timeout_duration;

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }

            match timeout(remaining, self.event_rx.recv()).await {
                Ok(Some(event)) => {
                    if predicate(&event) {
                        return Some(event);
                    }
                    // Not matching, continue waiting
                }
                Ok(None) => return None, // Channel closed
                Err(_) => return None,   // Timeout
            }
        }
    }

    /// Wait for sync to complete.
    ///
    /// Returns true if sync completed within the timeout, false otherwise.
    pub async fn wait_for_sync_complete(&mut self, timeout_duration: Duration) -> bool {
        self.wait_for_event(
            |e| matches!(e, NetworkEvent::SyncStateChanged(SyncState::Synced)),
            timeout_duration,
        )
        .await
        .is_some()
    }

    /// Wait for N peer connections.
    ///
    /// Returns true if N peers connected within the timeout, false otherwise.
    #[allow(dead_code)]
    pub async fn wait_for_n_peers(&mut self, n: usize, timeout_duration: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + timeout_duration;
        let mut _connected = 0;

        loop {
            // Check current peer count
            let current_peers = self.peer_count().await;
            if current_peers >= n {
                return true;
            }

            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }

            // Wait for a peer connection event
            match timeout(remaining, self.event_rx.recv()).await {
                Ok(Some(NetworkEvent::PeerConnected { .. })) => {
                    _connected += 1;
                    if self.peer_count().await >= n {
                        return true;
                    }
                }
                Ok(Some(_)) => {
                    // Other event, continue waiting
                }
                Ok(None) => return false, // Channel closed
                Err(_) => return false,   // Timeout
            }
        }
    }

    /// Submit a mined block to the network.
    pub async fn submit_block(&self, block: Block) -> Result<(), mpsc::error::SendError<Block>> {
        self.block_submitter.send(block).await
    }

    /// Mine a block with easy difficulty.
    ///
    /// This uses the test difficulty (0x40ffffff) which should find a block
    /// almost instantly.
    pub async fn mine_block(&self) -> Option<Block> {
        let blockchain = self.blockchain.read().await;
        let mempool = self.mempool.read().await;
        let stop = Arc::new(AtomicBool::new(false));

        match mine_block(&blockchain, &mempool, self.miner_address, stop) {
            MineResult::Success(block) => Some(block),
            _ => None,
        }
    }

    /// Get the number of transactions in the mempool.
    pub async fn mempool_size(&self) -> usize {
        self.mempool.read().await.len()
    }

    /// Find a mature UTXO that can be spent by this node.
    ///
    /// Returns None if no mature UTXOs are available.
    pub async fn find_mature_utxo(&self) -> Option<(OutPoint, u64)> {
        let blockchain = self.blockchain.read().await;
        let height = blockchain.height();
        let utxos = blockchain.utxos_for_address(&self.miner_address);

        for (outpoint, utxo) in utxos {
            // Coinbase outputs need COINBASE_MATURITY confirmations
            if !utxo.is_coinbase || height >= utxo.height + COINBASE_MATURITY {
                return Some((outpoint, utxo.output.amount));
            }
        }
        None
    }

    /// Create a signed transaction sending funds to another address.
    ///
    /// Returns None if no mature UTXOs are available or if the amount + fee exceeds available funds.
    /// The fee is calculated based on transaction size (MIN_RELAY_FEE = 1000 quanta per KB).
    pub async fn create_transaction(
        &self,
        to_address: Address,
        amount: u64,
    ) -> Option<Transaction> {
        let (outpoint, utxo_amount) = self.find_mature_utxo().await?;

        // Create transaction structure first to calculate size
        let inputs = vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: Box::new(self.keypair.0.clone()),
                signature: Box::new(ml_dsa_87::sign(&self.keypair.1, b"placeholder")),
            },
        )];

        // Start with just the payment output to estimate size
        let mut outputs = vec![TxOutput::p2pkh(amount, to_address)];

        let mut tx = Transaction::new(inputs.clone(), outputs.clone());

        // Sign with placeholder to get accurate size
        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let signature = ml_dsa_87::sign(&self.keypair.1, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: Box::new(self.keypair.0.clone()),
            signature: Box::new(signature),
        };

        // Calculate fee based on transaction size
        // fee_rate = fee / tx_size must be >= MIN_RELAY_FEE (1000)
        // Therefore: fee >= tx_size * 1000
        // Add extra buffer for change output which increases final tx size
        let tx_size = tx.to_bytes().len() as u64;
        let fee = (tx_size + 100) * 1000 + 10000; // Extra buffer for change output

        // Check if we have enough funds
        if utxo_amount < amount + fee {
            return None;
        }

        // Rebuild with change output if needed
        let change = utxo_amount - amount - fee;
        if change > 0 {
            outputs.push(TxOutput::p2pkh(change, self.miner_address));
        }

        let mut tx = Transaction::new(inputs, outputs);

        // Sign the transaction properly
        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let signature = ml_dsa_87::sign(&self.keypair.1, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: Box::new(self.keypair.0.clone()),
            signature: Box::new(signature),
        };

        Some(tx)
    }

    /// Add a transaction to this node's mempool.
    ///
    /// This adds the transaction locally but does not broadcast it.
    pub async fn add_to_mempool(
        &self,
        tx: Transaction,
    ) -> Result<(), pqcoin::mempool::MempoolError> {
        let blockchain = self.blockchain.read().await;
        let height = blockchain.height();
        let mut mempool = self.mempool.write().await;
        mempool.add(tx, &blockchain, height)?;
        Ok(())
    }

    /// Mine a block and submit it to the network.
    pub async fn mine_and_submit_block(&self) -> Option<Block> {
        // Mine a block
        let block = {
            let blockchain = self.blockchain.read().await;
            let mempool = self.mempool.read().await;
            let stop = Arc::new(AtomicBool::new(false));

            match mine_block(&blockchain, &mempool, self.miner_address, stop) {
                MineResult::Success(block) => block,
                _ => return None,
            }
        };

        // Add block to local blockchain first
        {
            let mut blockchain = self.blockchain.write().await;
            let mut mempool = self.mempool.write().await;
            if blockchain.add_block(block.clone()).is_err() {
                return None;
            }
            mempool.remove_confirmed(&block.transactions);
        }

        // Submit to network for propagation
        if self.block_submitter.send(block.clone()).await.is_err() {
            return None;
        }

        Some(block)
    }

    /// Block a socket address from connecting (for partition testing).
    pub async fn block_addr(&self, addr: SocketAddr) {
        let mut state = self.network_state.write().await;
        state.partition_blocklist.block(addr);
    }

    /// Clear all blocked addresses (for partition testing).
    pub async fn clear_blocklist(&self) {
        let mut state = self.network_state.write().await;
        state.partition_blocklist.clear();
    }

    /// Disconnect a peer by their socket address.
    ///
    /// Returns true if a peer was found and disconnected, false otherwise.
    pub async fn disconnect_addr(&self, addr: &SocketAddr) -> bool {
        let sender = {
            let state = self.network_state.read().await;
            state.get_peer_sender(addr)
        };

        if let Some(sender) = sender {
            let _ = sender.send(PeerCommand::Disconnect).await;
            true
        } else {
            false
        }
    }

    /// Gracefully shutdown the node.
    pub async fn shutdown(mut self) {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Wait for service to stop
        if let Some(handle) = self.service_handle.take() {
            let _ = timeout(Duration::from_secs(5), handle).await;
        }
    }

    /// Shutdown the node and return info needed to restart it.
    ///
    /// Returns RestartInfo containing the storage path, keypair, and state.
    /// The storage path can be used to restart the node from persistent state.
    pub async fn shutdown_for_restart(mut self) -> RestartInfo {
        // Get final state before shutdown
        let final_height = self.height().await;
        let final_tip = self.tip_hash().await;

        // Extract values we need (clone to avoid move issues with Drop)
        let keypair = self.keypair.clone();
        let miner_address = self.miner_address;
        let id = self.id;

        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }

        // Wait for service to stop
        if let Some(handle) = self.service_handle.take() {
            let _ = timeout(Duration::from_secs(5), handle).await;
        }

        // Small delay to ensure LMDB flushes
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Get storage path and prevent TempDir deletion
        let storage_path = match &self.storage_dir {
            StorageDir::Temp(temp) => temp.path().to_path_buf(),
            StorageDir::Persistent(path) => path.clone(),
        };

        // Replace storage_dir with Persistent to prevent deletion on drop
        let old_storage = std::mem::replace(
            &mut self.storage_dir,
            StorageDir::Persistent(storage_path.clone()),
        );
        if let StorageDir::Temp(temp) = old_storage {
            std::mem::forget(temp); // Leak to prevent deletion
        }

        RestartInfo {
            storage_path,
            keypair,
            miner_address,
            id,
            final_height,
            final_tip,
        }
    }
}

/// Information needed to restart a node from persistent storage.
pub struct RestartInfo {
    /// Path to the storage directory.
    pub storage_path: std::path::PathBuf,
    /// The node's keypair.
    pub keypair: (PublicKey, SecretKey),
    /// The node's miner address.
    pub miner_address: Address,
    /// The node's ID.
    pub id: usize,
    /// The blockchain height at shutdown.
    pub final_height: u64,
    /// The tip hash at shutdown.
    pub final_tip: Hash,
}

impl RestartInfo {
    /// Restart the node from this info.
    ///
    /// Consumes self and transfers ownership of the storage directory to the new node.
    pub async fn restart(
        mut self,
        port: u16,
        seed_peers: Vec<SocketAddr>,
        genesis: Block,
    ) -> Result<TestNode, Box<dyn std::error::Error + Send + Sync>> {
        // Take ownership of values before self is dropped
        let storage_path = std::mem::take(&mut self.storage_path);
        let keypair = std::mem::replace(&mut self.keypair, ml_dsa_87::keygen()); // Dummy replacement
        let miner_address = self.miner_address;
        let id = self.id;

        // Mark as consumed so Drop doesn't delete the directory
        self.storage_path = std::path::PathBuf::new(); // Empty path won't delete anything meaningful

        let storage_dir = StorageDir::Persistent(storage_path);
        TestNode::create_with_storage(
            id,
            port,
            seed_peers,
            genesis,
            storage_dir,
            keypair,
            miner_address,
        )
        .await
    }

    /// Restart the node without networking (for persistence verification).
    ///
    /// This creates an IsolatedNode that can be used to verify the persistent
    /// blockchain state before any sync happens. Call `start_networking()` on
    /// the returned node to begin syncing.
    pub async fn restart_isolated(
        mut self,
        port: u16,
        genesis: Block,
    ) -> Result<IsolatedNode, Box<dyn std::error::Error + Send + Sync>> {
        // Take ownership of values before self is dropped
        let storage_path = std::mem::take(&mut self.storage_path);
        let keypair = std::mem::replace(&mut self.keypair, ml_dsa_87::keygen()); // Dummy replacement
        let miner_address = self.miner_address;
        let id = self.id;

        // Mark as consumed so Drop doesn't delete the directory
        self.storage_path = std::path::PathBuf::new(); // Empty path won't delete anything meaningful

        let storage_dir = StorageDir::Persistent(storage_path);
        IsolatedNode::create_with_storage(id, port, genesis, storage_dir, keypair, miner_address)
            .await
    }
}

impl Drop for RestartInfo {
    fn drop(&mut self) {
        // Only clean up if storage_path is non-empty (not consumed by restart)
        if !self.storage_path.as_os_str().is_empty() {
            let _ = std::fs::remove_dir_all(&self.storage_path);
        }
    }
}

impl Drop for TestNode {
    fn drop(&mut self) {
        // Send shutdown signal if not already sent
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// A node created without networking, for staged startup.
///
/// This allows verifying persistent state before networking starts,
/// eliminating race conditions where sync completes before assertions.
pub struct IsolatedNode {
    /// Node identifier for debugging.
    pub id: usize,
    /// Reference to the blockchain.
    pub blockchain: Arc<RwLock<Blockchain>>,
    /// Storage directory (kept alive for the node's lifetime).
    storage_dir: StorageDir,
    /// Miner address for this node.
    pub miner_address: Address,
    /// Keypair for signing transactions.
    pub keypair: (PublicKey, SecretKey),
    /// The listen address to use when networking starts.
    listen_addr: SocketAddr,
}

impl IsolatedNode {
    /// Create a node with blockchain loaded but no networking.
    ///
    /// This allows querying the blockchain state without any possibility
    /// of sync happening in the background.
    #[allow(dead_code)]
    pub async fn create(
        id: usize,
        port: u16,
        genesis: Block,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create temporary storage directory
        let storage_dir = StorageDir::Temp(TempDir::new()?);

        // Generate keypair for this node
        let keypair = ml_dsa_87::keygen();
        let miner_address = Address::from_public_key(&keypair.0);

        Self::create_with_storage(id, port, genesis, storage_dir, keypair, miner_address).await
    }

    /// Create an isolated node with specific storage and keypair.
    pub async fn create_with_storage(
        id: usize,
        port: u16,
        genesis: Block,
        storage_dir: StorageDir,
        keypair: (PublicKey, SecretKey),
        miner_address: Address,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Create blockchain with persistent storage (like production)
        let blockchain = Blockchain::open(
            storage_dir.path(),
            genesis,
            2016,       // difficulty_adjustment_interval
            600,        // target_block_time
            50_000_000, // initial_reward
            210_000,    // halving_interval
        )?;
        let blockchain = Arc::new(RwLock::new(blockchain));

        let listen_addr: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();

        Ok(Self {
            id,
            blockchain,
            storage_dir,
            miner_address,
            keypair,
            listen_addr,
        })
    }

    /// Get the current blockchain height.
    pub async fn height(&self) -> u64 {
        self.blockchain.read().await.height()
    }

    /// Get the tip hash.
    pub async fn tip_hash(&self) -> Hash {
        self.blockchain.read().await.tip_hash()
    }

    /// Get the listen address that will be used when networking starts.
    #[allow(dead_code)]
    pub fn addr(&self) -> SocketAddr {
        self.listen_addr
    }

    /// Start networking and convert to a full TestNode.
    ///
    /// This consumes the IsolatedNode and starts the network service,
    /// connecting to the specified seed peers.
    pub async fn start_networking(
        self,
        seed_peers: Vec<SocketAddr>,
    ) -> Result<TestNode, Box<dyn std::error::Error + Send + Sync>> {
        // Configure network
        let config = NetworkConfig {
            listen_addr: self.listen_addr,
            max_peers: 50,
            max_outbound: 25,
            seed_peers,
        };

        // Create network service with existing blockchain and mempool
        let mut service = NetworkService::new(self.blockchain.clone(), config);

        // Take receivers and shared state before starting
        let event_rx = service
            .take_event_receiver()
            .expect("event receiver already taken");
        let block_submitter = service.block_submitter();
        let network_state = service.state();

        // Replace our mempool with the service's mempool (they need to share the same one)
        let mempool = service.mempool();

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // Start the service
        let service_handle =
            tokio::spawn(async move { service.run_with_shutdown(shutdown_rx).await });

        // Wait for service to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        Ok(TestNode {
            id: self.id,
            blockchain: self.blockchain,
            mempool,
            network_state,
            listen_addr: self.listen_addr,
            event_rx,
            block_submitter,
            shutdown_tx: Some(shutdown_tx),
            service_handle: Some(service_handle),
            storage_dir: self.storage_dir,
            miner_address: self.miner_address,
            keypair: self.keypair,
        })
    }
}
