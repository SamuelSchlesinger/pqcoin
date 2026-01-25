//! Network service - main coordinator for P2P networking.
//!
//! The `NetworkService` manages:
//! - Listening for incoming connections
//! - Connecting to peers
//! - Message routing between peers and local handlers
//! - Block and transaction relay

mod defense;
mod events;
mod handlers;
mod state;

// Re-export public types
pub use events::{NetworkError, NetworkEvent};

use crate::blockchain::{Block, Blockchain, Transaction};
use crate::constants::{DEFAULT_PORT, MAX_OUTBOUND, MAX_PEERS, PING_INTERVAL_SECS};
use crate::mempool::Mempool;
use crate::network::message::{InvItem, Message};
use crate::network::peer::Peer;
use crate::network::sync::{SyncManager, SyncState};

use handlers::{broadcast, handle_peer_message, send_to_peer};
use state::{NetworkState, PeerCommand, PeerInfo, PeerMessage};

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, mpsc};
use tokio::time::interval;

/// Network configuration.
#[derive(Debug, Clone)]
pub struct NetworkConfig {
    /// Address to listen on.
    pub listen_addr: SocketAddr,
    /// Maximum number of peers.
    pub max_peers: usize,
    /// Maximum number of outbound connections.
    pub max_outbound: usize,
    /// Initial peers to connect to.
    pub seed_peers: Vec<SocketAddr>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            listen_addr: format!("0.0.0.0:{}", DEFAULT_PORT).parse().unwrap(),
            max_peers: MAX_PEERS,
            max_outbound: MAX_OUTBOUND,
            seed_peers: Vec::new(),
        }
    }
}

/// The main network service.
pub struct NetworkService {
    /// Configuration.
    config: NetworkConfig,
    /// Reference to the blockchain.
    blockchain: Arc<RwLock<Blockchain>>,
    /// Transaction mempool.
    mempool: Arc<RwLock<Mempool>>,
    /// Sync manager.
    sync: Arc<RwLock<SyncManager>>,
    /// Shared network state.
    state: Arc<RwLock<NetworkState>>,
    /// Event channel sender.
    event_tx: mpsc::Sender<NetworkEvent>,
    /// Event channel receiver (for consumers).
    event_rx: Option<mpsc::Receiver<NetworkEvent>>,
    /// Channel for messages from peer tasks.
    peer_msg_tx: mpsc::Sender<PeerMessage>,
    /// Channel for receiving messages from peer tasks.
    peer_msg_rx: Option<mpsc::Receiver<PeerMessage>>,
    /// Channel for submitting locally mined blocks.
    block_submit_tx: mpsc::Sender<Block>,
    /// Receiver for locally mined blocks.
    block_submit_rx: Option<mpsc::Receiver<Block>>,
    /// Next peer ID.
    next_peer_id: Arc<AtomicU64>,
}

impl NetworkService {
    /// Create a new network service.
    pub fn new(blockchain: Arc<RwLock<Blockchain>>, config: NetworkConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);
        let (peer_msg_tx, peer_msg_rx) = mpsc::channel(1000);
        let (block_submit_tx, block_submit_rx) = mpsc::channel(100);

        let sync = Arc::new(RwLock::new(SyncManager::new(blockchain.clone())));
        let mempool = Arc::new(RwLock::new(Mempool::new()));

        Self {
            config,
            blockchain,
            mempool,
            sync,
            state: Arc::new(RwLock::new(NetworkState::new())),
            event_tx,
            event_rx: Some(event_rx),
            peer_msg_tx,
            peer_msg_rx: Some(peer_msg_rx),
            block_submit_tx,
            block_submit_rx: Some(block_submit_rx),
            next_peer_id: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Get a sender for submitting locally mined blocks.
    pub fn block_submitter(&self) -> mpsc::Sender<Block> {
        self.block_submit_tx.clone()
    }

    /// Get a reference to the mempool.
    pub fn mempool(&self) -> Arc<RwLock<Mempool>> {
        self.mempool.clone()
    }

    /// Get a reference to the blockchain.
    pub fn blockchain(&self) -> Arc<RwLock<Blockchain>> {
        self.blockchain.clone()
    }

    /// Take the event receiver (can only be called once).
    pub fn take_event_receiver(&mut self) -> Option<mpsc::Receiver<NetworkEvent>> {
        self.event_rx.take()
    }

    /// Get the current number of connected peers.
    pub async fn peer_count(&self) -> usize {
        self.state.read().await.peers.len()
    }

    /// Get the current sync state.
    pub async fn sync_state(&self) -> SyncState {
        self.sync.read().await.state()
    }

    /// Run the network service.
    pub async fn run(&mut self) -> Result<(), NetworkError> {
        // Start listening
        let listener = TcpListener::bind(self.config.listen_addr).await?;
        tracing::info!(addr = %self.config.listen_addr, "listening for connections");

        // Take the peer message receiver
        let mut peer_msg_rx = self.peer_msg_rx.take().expect("run called twice");

        // Take the block submit receiver
        let mut block_submit_rx = self.block_submit_rx.take().expect("run called twice");

        // Connect to seed peers
        for addr in self.config.seed_peers.clone() {
            let _ = self.connect(addr).await;
        }

        // Periodic tasks
        let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
        let mut sync_interval = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                // Accept incoming connections
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            if let Err(e) = self.handle_incoming(stream, addr).await {
                                tracing::debug!(addr = %addr, error = %e, "failed to accept connection");
                            }
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "accept error");
                        }
                    }
                }

                // Handle messages from peer tasks
                Some(msg) = peer_msg_rx.recv() => {
                    handle_peer_message(
                        msg,
                        &self.state,
                        &self.blockchain,
                        &self.mempool,
                        &self.sync,
                        &self.event_tx,
                    ).await;
                }

                // Handle locally mined blocks
                Some(block) = block_submit_rx.recv() => {
                    self.handle_mined_block(block).await;
                }

                // Periodic ping
                _ = ping_interval.tick() => {
                    self.send_pings().await;
                }

                // Periodic sync check
                _ = sync_interval.tick() => {
                    self.check_sync().await;
                }
            }
        }
    }

    /// Connect to a peer.
    pub async fn connect(&self, addr: SocketAddr) -> Result<u64, NetworkError> {
        // Check if already connected
        {
            let state = self.state.read().await;
            if state.connected_addrs.contains_key(&addr) {
                return Err(NetworkError::AlreadyConnected);
            }
            if state.peers.len() >= self.config.max_peers {
                return Err(NetworkError::MaxPeersReached);
            }
            if state.outbound_count >= self.config.max_outbound {
                return Err(NetworkError::MaxPeersReached);
            }
        }

        tracing::info!(addr = %addr, "connecting to peer");

        let stream = TcpStream::connect(addr).await?;
        let peer_id = self.next_peer_id.fetch_add(1, Ordering::SeqCst);

        self.spawn_peer_task(peer_id, addr, stream, true).await?;

        Ok(peer_id)
    }

    /// Handle an incoming connection.
    async fn handle_incoming(
        &self,
        stream: TcpStream,
        addr: SocketAddr,
    ) -> Result<u64, NetworkError> {
        let ip = addr.ip();

        {
            let mut state = self.state.write().await;

            // Check if IP is banned
            if state.ban_list.is_banned(&ip) {
                tracing::debug!(addr = %addr, "connection from banned IP rejected");
                return Err(NetworkError::MaxPeersReached);
            }

            // Check rate limiting
            if !state.rate_limiter.check_allowed(ip) {
                tracing::debug!(addr = %addr, "connection rate limited");
                return Err(NetworkError::MaxPeersReached);
            }

            // Check subnet diversity (eclipse attack protection)
            if !state.subnet_limiter.check_allowed(&ip) {
                tracing::debug!(addr = %addr, "connection rejected: subnet limit reached");
                return Err(NetworkError::MaxPeersReached);
            }

            if state.connected_addrs.contains_key(&addr) {
                return Err(NetworkError::AlreadyConnected);
            }
            if state.peers.len() >= self.config.max_peers {
                return Err(NetworkError::MaxPeersReached);
            }

            // Record the connection for rate limiting and subnet tracking
            state.rate_limiter.record_connection(ip);
            state.subnet_limiter.record_connection(&ip);
        }

        tracing::info!(addr = %addr, "incoming connection");

        let peer_id = self.next_peer_id.fetch_add(1, Ordering::SeqCst);
        self.spawn_peer_task(peer_id, addr, stream, false).await?;

        Ok(peer_id)
    }

    /// Spawn a task to handle a peer connection.
    async fn spawn_peer_task(
        &self,
        peer_id: u64,
        addr: SocketAddr,
        stream: TcpStream,
        outbound: bool,
    ) -> Result<(), NetworkError> {
        let blockchain = self.blockchain.clone();
        let local_addr = self.config.listen_addr;
        let local_height = blockchain.read().await.height();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<PeerCommand>(100);
        let peer_msg_tx = self.peer_msg_tx.clone();

        // Register peer
        {
            let mut state = self.state.write().await;
            state.peers.insert(
                peer_id,
                PeerInfo {
                    addr,
                    height: 0,
                    outbound,
                },
            );
            state.connected_addrs.insert(addr, peer_id);
            state.peer_senders.insert(peer_id, cmd_tx);
            if outbound {
                state.outbound_count += 1;
            }
        }

        // Spawn peer task
        tokio::spawn(async move {
            let mut peer = Peer::new(peer_id, addr, stream, outbound, local_addr, local_height);

            // Perform handshake
            if let Err(e) = peer.handshake().await {
                tracing::warn!(peer_id = peer_id, addr = %addr, error = %e, "handshake failed");
                let _ = peer_msg_tx
                    .send(PeerMessage::Disconnected {
                        peer_id,
                        error: Some(e.to_string()),
                    })
                    .await;
                return;
            }

            // Get peer's height from handshake
            let peer_height = peer.info().map(|i| i.height).unwrap_or(0);
            tracing::info!(peer_id = peer_id, addr = %addr, height = peer_height, "peer connected");

            // Notify service of successful handshake with peer's height
            let _ = peer_msg_tx
                .send(PeerMessage::HandshakeComplete {
                    peer_id,
                    height: peer_height,
                })
                .await;

            // Message loop
            loop {
                tokio::select! {
                    // Receive command from service
                    Some(cmd) = cmd_rx.recv() => {
                        match cmd {
                            PeerCommand::Send(msg) => {
                                if let Err(e) = peer.send_message(&msg).await {
                                    tracing::debug!(peer_id = peer_id, error = %e, "send failed");
                                    break;
                                }
                            }
                            PeerCommand::Disconnect => {
                                break;
                            }
                        }
                    }

                    // Receive message from peer
                    result = peer.receive_message() => {
                        match result {
                            Ok(msg) => {
                                if peer_msg_tx
                                    .send(PeerMessage::Received { peer_id, message: msg })
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::debug!(peer_id = peer_id, error = %e, "receive failed");
                                break;
                            }
                        }
                    }
                }
            }

            peer.disconnect().await;
            let _ = peer_msg_tx
                .send(PeerMessage::Disconnected {
                    peer_id,
                    error: None,
                })
                .await;
        });

        // Send connected event
        let _ = self
            .event_tx
            .send(NetworkEvent::PeerConnected { peer_id, addr })
            .await;

        Ok(())
    }

    /// Send a message to a specific peer.
    async fn send_to_peer(&self, peer_id: u64, message: Message) {
        send_to_peer(peer_id, message, &self.state).await;
    }

    /// Broadcast a message to all peers.
    pub async fn broadcast(&self, message: Message) {
        broadcast(message, &self.state).await;
    }

    /// Ban a peer and disconnect them.
    ///
    /// The peer's IP address will be banned for the configured duration,
    /// preventing future connections from that IP.
    pub async fn ban_peer(&self, peer_id: u64, reason: &str) {
        let (addr, sender) = {
            let mut state = self.state.write().await;
            let peer_info = state.peers.get(&peer_id).cloned();
            if let Some(info) = peer_info {
                let ip = info.addr.ip();
                state.ban_list.ban(ip);
                let sender = state.peer_senders.get(&peer_id).cloned();
                (Some(info.addr), sender)
            } else {
                (None, None)
            }
        };

        if let Some(addr) = addr {
            tracing::warn!(peer_id = peer_id, addr = %addr, reason = reason, "banning peer");
            // Disconnect the peer
            if let Some(sender) = sender {
                let _ = sender.send(PeerCommand::Disconnect).await;
            }
        }
    }

    /// Broadcast a new block to all peers.
    pub async fn broadcast_block(&self, block: Block) {
        self.broadcast(Message::Inv {
            items: vec![InvItem::block(block.hash())],
        })
        .await;
    }

    /// Broadcast a new transaction to all peers.
    pub async fn broadcast_tx(&self, tx: Transaction) {
        self.broadcast(Message::Inv {
            items: vec![InvItem::tx(tx.txid())],
        })
        .await;
    }

    /// Handle a locally mined block - broadcast it to peers.
    async fn handle_mined_block(&self, block: Block) {
        let hash = block.hash();
        let height = {
            let blockchain = self.blockchain.read().await;
            blockchain.height()
        };

        tracing::info!(
            hash = %hash.to_hex()[..16],
            height = height,
            "broadcasting mined block to peers"
        );

        // Emit event
        let _ = self
            .event_tx
            .send(NetworkEvent::NewBlock(block.clone()))
            .await;

        // Broadcast inventory to all peers
        self.broadcast(Message::Inv {
            items: vec![InvItem::block(hash)],
        })
        .await;
    }

    /// Submit a local transaction to the mempool and broadcast it.
    pub async fn submit_transaction(
        &self,
        tx: Transaction,
    ) -> Result<(), crate::mempool::MempoolError> {
        let txid = tx.txid();

        // Add to mempool
        {
            let blockchain = self.blockchain.read().await;
            let current_height = blockchain.height();
            let mut mempool = self.mempool.write().await;
            mempool.add(tx.clone(), &blockchain, current_height)?;
        }

        tracing::info!(
            txid = %txid.to_hex()[..16],
            "submitted local transaction"
        );

        // Emit event
        let _ = self
            .event_tx
            .send(NetworkEvent::NewTransaction(tx.clone()))
            .await;

        // Broadcast to peers
        self.broadcast(Message::Inv {
            items: vec![InvItem::tx(txid)],
        })
        .await;

        Ok(())
    }

    /// Submit a mined block to the blockchain and broadcast it.
    pub async fn submit_block(
        &self,
        block: Block,
    ) -> Result<(), crate::blockchain::BlockchainError> {
        let hash = block.hash();

        // IMPORTANT: Hold both locks together to prevent race conditions
        // This ensures no transactions can be added to mempool that conflict
        // with the newly added block
        let new_height = {
            let mut blockchain = self.blockchain.write().await;
            let mut mempool = self.mempool.write().await;

            blockchain.add_block(block.clone())?;
            mempool.remove_confirmed(&block.transactions);

            blockchain.height()
        };
        tracing::info!(
            hash = %hash.to_hex()[..16],
            height = new_height,
            txs = block.transactions.len(),
            "submitted mined block"
        );

        // Emit event
        let _ = self
            .event_tx
            .send(NetworkEvent::NewBlock(block.clone()))
            .await;

        // Broadcast to peers
        self.broadcast(Message::Inv {
            items: vec![InvItem::block(hash)],
        })
        .await;

        Ok(())
    }

    /// Send ping to all connected peers.
    async fn send_pings(&self) {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;

        tracing::debug!(nonce = nonce, "sending ping to all peers");
        self.broadcast(Message::Ping(nonce)).await;
    }

    /// Check if we need to sync and start syncing if necessary.
    async fn check_sync(&self) {
        let mut sync = self.sync.write().await;

        // Check for timed out block requests
        let timed_out = sync.check_timeouts();
        for (peer_id, hash) in timed_out {
            tracing::warn!(
                peer_id = peer_id,
                hash = %hash.to_hex()[..16],
                "block download timed out"
            );
            // Could increase ban score for peer here
        }

        let current_state = sync.state();

        match current_state {
            SyncState::Idle => {
                // Check if any peer has a higher chain
                let state = self.state.read().await;
                let our_height = self.blockchain.read().await.height();

                // Find the best peer to sync from
                let mut best_peer: Option<(u64, u64)> = None;
                for (&peer_id, info) in &state.peers {
                    if info.height > our_height {
                        sync.update_peer_height(peer_id, info.height);
                        if best_peer.is_none() || info.height > best_peer.unwrap().1 {
                            best_peer = Some((peer_id, info.height));
                        }
                    }
                }

                // Start header download from best peer
                if let Some((peer_id, _)) = best_peer {
                    drop(state);
                    let request = sync.create_get_headers_message().await;
                    drop(sync);
                    self.send_to_peer(peer_id, request).await;
                }
            }
            SyncState::DownloadingHeaders => {
                // Headers are being downloaded, nothing to do here
            }
            SyncState::DownloadingBlocks => {
                // Check if we need to request more blocks
                let state = self.state.read().await;
                if let Some((&peer_id, _)) = state.peers.iter().next() {
                    drop(state);
                    if let Some(request) = sync.get_blocks_to_download(peer_id) {
                        drop(sync);
                        self.send_to_peer(peer_id, request).await;
                    }
                }
            }
            SyncState::Synced => {
                // Already synced, nothing to do
            }
        }
    }

    /// Disconnect a peer.
    pub async fn disconnect_peer(&self, peer_id: u64) {
        let state = self.state.read().await;
        if let Some(sender) = state.peer_senders.get(&peer_id) {
            let _ = sender.send(PeerCommand::Disconnect).await;
        }
    }

    /// Get list of connected peer addresses.
    pub async fn connected_peers(&self) -> Vec<(u64, SocketAddr)> {
        let state = self.state.read().await;
        state
            .peers
            .iter()
            .map(|(&id, info)| (id, info.addr))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::{Address, create_genesis_block};
    use crate::crypto::ml_dsa_87;

    fn create_test_blockchain() -> Arc<RwLock<Blockchain>> {
        let (pk, _) = ml_dsa_87::keygen();
        let addr = Address::from_public_key(&pk);
        let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, addr);
        let blockchain = Blockchain::new(genesis, 2016, 600, 50_000_000, 210_000);
        Arc::new(RwLock::new(blockchain))
    }

    #[tokio::test]
    async fn test_network_service_creation() {
        let blockchain = create_test_blockchain();
        let config = NetworkConfig::default();
        let service = NetworkService::new(blockchain, config);
        assert_eq!(service.peer_count().await, 0);
    }
}
