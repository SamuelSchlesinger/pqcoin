//! Network service - main coordinator for P2P networking.
//!
//! The `NetworkService` manages:
//! - Listening for incoming connections
//! - Connecting to peers
//! - Message routing between peers and local handlers
//! - Block and transaction relay

mod addrman;
mod defense;
mod events;
mod handlers;
mod state;

// Re-export public types
pub use events::{NetworkError, NetworkEvent};
pub use state::{ConnectedPeerInfo, NetworkState, PeerCommand};

use crate::blockchain::{Block, Blockchain, Transaction};
use crate::constants::{
    ADDR_FETCH_INTERVAL_SECS, CONNECTION_RETRY_INTERVAL_SECS, DEFAULT_PORT, GETADDR_DELAY_SECS,
    MAX_OUTBOUND, MAX_PEERS, PEER_ROTATION_INTERVAL_SECS, PING_INTERVAL_SECS,
};
use crate::mempool::Mempool;
use crate::network::message::{InvItem, Message, Services};
use crate::network::peer::Peer;
use crate::network::sync::{SyncManager, SyncState};

use handlers::{broadcast, handle_peer_message, send_to_peer};
use state::PeerMessage;

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
            // COMPILE-TIME: "0.0.0.0:{port}" is always a valid socket address format.
            listen_addr: format!("0.0.0.0:{DEFAULT_PORT}").parse().unwrap(),
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
    /// Actual bound address (set after binding to a port).
    local_addr: Option<SocketAddr>,
}

impl NetworkService {
    /// Create a new network service.
    pub fn new(blockchain: Arc<RwLock<Blockchain>>, config: NetworkConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1000);
        let (peer_msg_tx, peer_msg_rx) = mpsc::channel(1000);
        let (block_submit_tx, block_submit_rx) = mpsc::channel(100);

        let sync = Arc::new(RwLock::new(SyncManager::new(
            blockchain.clone(),
            event_tx.clone(),
        )));
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
            local_addr: None,
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

    /// Get a reference to the network state for peer count queries.
    ///
    /// This allows external code to query the peer count even after the service
    /// is moved into a spawned task.
    pub fn state(&self) -> Arc<RwLock<NetworkState>> {
        self.state.clone()
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

    /// Get the actual bound local address.
    ///
    /// This is useful when binding to port 0 to get an ephemeral port.
    /// Returns `None` if the service hasn't started running yet.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.local_addr
    }

    /// Run the network service.
    pub async fn run(&mut self) -> Result<(), NetworkError> {
        // Start listening
        let listener = TcpListener::bind(self.config.listen_addr).await?;
        self.local_addr = Some(listener.local_addr()?);
        tracing::info!(addr = %self.local_addr.unwrap(), "listening for connections");

        // Take the peer message receiver
        // INVARIANT: peer_msg_rx is Some until run() is called, then taken exactly once.
        let mut peer_msg_rx = self.peer_msg_rx.take().expect("run called twice");

        // Take the block submit receiver
        // INVARIANT: block_submit_rx is Some until run() is called, then taken exactly once.
        let mut block_submit_rx = self.block_submit_rx.take().expect("run called twice");

        // Connect to seed peers and add them to addr_manager
        for addr in self.config.seed_peers.clone() {
            // Add seed peers to address manager
            {
                // INVARIANT: SystemTime::now() is always after UNIX_EPOCH.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let mut state = self.state.write().await;
                state
                    .addr_manager
                    .add_addr(addr, Services::NODE_NETWORK, now, None);
            }
            let _ = self.connect(addr).await;
        }

        // Register our local address
        {
            let mut state = self.state.write().await;
            if let Some(local) = self.local_addr {
                state.addr_manager.add_local_addr(local);
            }
        }

        // Periodic tasks
        let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
        let mut sync_interval = interval(Duration::from_secs(1));
        let mut peer_rotation_interval = interval(Duration::from_secs(PEER_ROTATION_INTERVAL_SECS));
        let mut addr_fetch_interval = interval(Duration::from_secs(ADDR_FETCH_INTERVAL_SECS));
        let mut connection_retry_interval =
            interval(Duration::from_secs(CONNECTION_RETRY_INTERVAL_SECS));
        let mut getaddr_check_interval = interval(Duration::from_secs(1));

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

                // Periodic peer rotation
                _ = peer_rotation_interval.tick() => {
                    self.rotate_random_peer().await;
                }

                // Periodic address fetch
                _ = addr_fetch_interval.tick() => {
                    self.fetch_addrs_from_random_peer().await;
                }

                // Periodic connection retry
                _ = connection_retry_interval.tick() => {
                    self.try_fill_outbound_slots().await;
                }

                // Check for pending GetAddr messages
                _ = getaddr_check_interval.tick() => {
                    self.send_pending_getaddr().await;
                }
            }
        }
    }

    /// Run the network service with a shutdown signal.
    ///
    /// This is the same as `run()` but accepts a shutdown signal that can be used
    /// to gracefully stop the service. Useful for testing.
    pub async fn run_with_shutdown(
        &mut self,
        mut shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), NetworkError> {
        // Start listening
        let listener = TcpListener::bind(self.config.listen_addr).await?;
        self.local_addr = Some(listener.local_addr()?);
        tracing::info!(addr = %self.local_addr.unwrap(), "listening for connections");

        // Take the peer message receiver
        // INVARIANT: peer_msg_rx is Some until run() is called, then taken exactly once.
        let mut peer_msg_rx = self.peer_msg_rx.take().expect("run called twice");

        // Take the block submit receiver
        // INVARIANT: block_submit_rx is Some until run() is called, then taken exactly once.
        let mut block_submit_rx = self.block_submit_rx.take().expect("run called twice");

        // Connect to seed peers and add them to addr_manager
        for addr in self.config.seed_peers.clone() {
            // Add seed peers to address manager
            {
                // INVARIANT: SystemTime::now() is always after UNIX_EPOCH.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let mut state = self.state.write().await;
                state
                    .addr_manager
                    .add_addr(addr, Services::NODE_NETWORK, now, None);
            }
            let _ = self.connect(addr).await;
        }

        // Register our local address
        {
            let mut state = self.state.write().await;
            if let Some(local) = self.local_addr {
                state.addr_manager.add_local_addr(local);
            }
        }

        // Periodic tasks
        let mut ping_interval = interval(Duration::from_secs(PING_INTERVAL_SECS));
        let mut sync_interval = interval(Duration::from_secs(1));
        let mut peer_rotation_interval = interval(Duration::from_secs(PEER_ROTATION_INTERVAL_SECS));
        let mut addr_fetch_interval = interval(Duration::from_secs(ADDR_FETCH_INTERVAL_SECS));
        let mut connection_retry_interval =
            interval(Duration::from_secs(CONNECTION_RETRY_INTERVAL_SECS));
        let mut getaddr_check_interval = interval(Duration::from_secs(1));

        loop {
            tokio::select! {
                // Shutdown signal
                _ = &mut shutdown => {
                    tracing::info!("shutdown signal received");
                    return Ok(());
                }

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

                // Periodic peer rotation
                _ = peer_rotation_interval.tick() => {
                    self.rotate_random_peer().await;
                }

                // Periodic address fetch
                _ = addr_fetch_interval.tick() => {
                    self.fetch_addrs_from_random_peer().await;
                }

                // Periodic connection retry
                _ = connection_retry_interval.tick() => {
                    self.try_fill_outbound_slots().await;
                }

                // Check for pending GetAddr messages
                _ = getaddr_check_interval.tick() => {
                    self.send_pending_getaddr().await;
                }
            }
        }
    }

    /// Connect to a peer.
    pub async fn connect(&self, addr: SocketAddr) -> Result<u64, NetworkError> {
        // Check if already connected
        {
            let state = self.state.read().await;

            // Check partition blocklist
            if state.partition_blocklist.is_blocked(&addr) {
                tracing::debug!(addr = %addr, "outbound connection blocked by partition blocklist");
                return Err(NetworkError::MaxPeersReached);
            }

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

            // Check partition blocklist
            if state.partition_blocklist.is_blocked(&addr) {
                tracing::debug!(addr = %addr, "incoming connection blocked by partition blocklist");
                return Err(NetworkError::MaxPeersReached);
            }

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
                ConnectedPeerInfo {
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
        // INVARIANT: SystemTime::now() is always after UNIX_EPOCH.
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
                if let Some((peer_id, peer_height)) = best_peer {
                    drop(state);
                    tracing::info!(
                        peer_id = peer_id,
                        peer_height = peer_height,
                        our_height = our_height,
                        "initiating sync with peer"
                    );
                    // Transition to DownloadingHeaders to prevent duplicate requests
                    sync.set_state(SyncState::DownloadingHeaders);
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
                // Pick the peer with the highest chain to ensure they have the blocks we need
                let state = self.state.read().await;
                let our_height = self.blockchain.read().await.height();
                let best_peer = state
                    .peers
                    .iter()
                    .filter(|(_, info)| info.height > our_height)
                    .max_by_key(|(_, info)| info.height)
                    .map(|(&id, _)| id);

                if let Some(peer_id) = best_peer {
                    drop(state);
                    if let Some(request) = sync.get_blocks_to_download(peer_id) {
                        drop(sync);
                        self.send_to_peer(peer_id, request).await;
                    }
                }
            }
            SyncState::Synced => {
                // Check if any peer has gotten ahead of us (e.g., new blocks mined)
                // If so, transition back to Idle to restart sync
                let state = self.state.read().await;
                let our_height = self.blockchain.read().await.height();

                let best_peer_height = state
                    .peers
                    .values()
                    .map(|info| info.height)
                    .max()
                    .unwrap_or(0);

                if best_peer_height > our_height {
                    drop(state);
                    tracing::info!(
                        our_height = our_height,
                        best_peer_height = best_peer_height,
                        "peer ahead, restarting sync"
                    );
                    sync.set_state(SyncState::Idle);
                }
            }
        }
    }

    /// Rotate one random outbound peer (disconnect + try new).
    ///
    /// This prevents eclipse attacks by ensuring peer diversity over time.
    /// Uses random selection rather than performance-based to avoid fingerprinting.
    async fn rotate_random_peer(&self) {
        // Collect info and pick random peer while holding lock briefly
        let peer_to_disconnect = {
            let state = self.state.read().await;

            // Only rotate if we have some outbound connections
            if state.outbound_count == 0 {
                return;
            }

            // Collect outbound peers
            let outbound_peers: Vec<(u64, std::net::SocketAddr)> = state
                .peers
                .iter()
                .filter(|(_, info)| info.outbound)
                .map(|(&id, info)| (id, info.addr))
                .collect();

            if outbound_peers.is_empty() {
                return;
            }

            // Check if we have any addresses to connect to
            let addr_count = state.addr_manager.addr_count();
            if addr_count <= 1 {
                // Not enough known addresses to rotate
                return;
            }

            // Pick a random outbound peer to disconnect
            let idx = rand::random::<usize>() % outbound_peers.len();
            outbound_peers[idx]
        };

        let (peer_id, addr) = peer_to_disconnect;

        tracing::debug!(
            peer_id = peer_id,
            addr = %addr,
            "rotating outbound peer"
        );

        // Disconnect the peer
        self.disconnect_peer(peer_id).await;

        // Try to connect to a new peer
        // The try_fill_outbound_slots will handle this on next tick
    }

    /// Request addresses from a random connected peer.
    async fn fetch_addrs_from_random_peer(&self) {
        let peer_id = {
            let state = self.state.read().await;

            if state.peers.is_empty() {
                return;
            }

            // Pick a random peer
            let peer_ids: Vec<u64> = state.peers.keys().cloned().collect();
            let idx = rand::random::<usize>() % peer_ids.len();
            peer_ids[idx]
        };

        tracing::debug!(peer_id = peer_id, "requesting addresses from peer");
        self.send_to_peer(peer_id, Message::GetAddr).await;
    }

    /// Fill empty outbound slots with new connections.
    async fn try_fill_outbound_slots(&self) {
        let (available_slots, connected_addrs) = {
            let state = self.state.read().await;
            let available = self
                .config
                .max_outbound
                .saturating_sub(state.outbound_count);
            let connected: std::collections::HashSet<std::net::SocketAddr> =
                state.connected_addrs.keys().cloned().collect();
            (available, connected)
        };

        if available_slots == 0 {
            return;
        }

        // Try to fill available slots
        for _ in 0..available_slots {
            let addr = {
                let mut state = self.state.write().await;
                state.addr_manager.get_random_addr(&connected_addrs)
            };

            if let Some(addr) = addr {
                // Mark as in progress
                {
                    let mut state = self.state.write().await;
                    state.addr_manager.mark_in_progress(&addr);
                }

                tracing::debug!(addr = %addr, "attempting new outbound connection");

                match self.connect(addr).await {
                    Ok(peer_id) => {
                        tracing::debug!(
                            peer_id = peer_id,
                            addr = %addr,
                            "new outbound connection established"
                        );
                        // mark_good will be called when handshake completes
                    }
                    Err(e) => {
                        tracing::debug!(
                            addr = %addr,
                            error = %e,
                            "failed to connect to address"
                        );
                        let mut state = self.state.write().await;
                        state.addr_manager.mark_attempt_failed(&addr);
                    }
                }
            } else {
                // No more addresses available
                break;
            }
        }
    }

    /// Send GetAddr to peers that completed handshake after the delay.
    async fn send_pending_getaddr(&self) {
        let now = std::time::Instant::now();
        let delay = Duration::from_secs(GETADDR_DELAY_SECS);

        let peers_to_send: Vec<u64> = {
            let state = self.state.read().await;
            state
                .pending_getaddr
                .iter()
                .filter(|(_, time)| now.duration_since(**time) >= delay)
                .map(|(peer_id, _)| *peer_id)
                .collect()
        };

        if peers_to_send.is_empty() {
            return;
        }

        // Remove from pending and send GetAddr
        {
            let mut state = self.state.write().await;
            for peer_id in &peers_to_send {
                state.pending_getaddr.remove(peer_id);
            }
        }

        for peer_id in peers_to_send {
            tracing::debug!(peer_id = peer_id, "sending GetAddr after handshake");
            self.send_to_peer(peer_id, Message::GetAddr).await;
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

    /// Block a socket address from connecting (for partition testing).
    pub async fn block_addr(&self, addr: SocketAddr) {
        let mut state = self.state.write().await;
        state.partition_blocklist.block(addr);
        tracing::debug!(addr = %addr, "added to partition blocklist");
    }

    /// Unblock a socket address (for partition testing).
    pub async fn unblock_addr(&self, addr: &SocketAddr) {
        let mut state = self.state.write().await;
        state.partition_blocklist.unblock(addr);
        tracing::debug!(addr = %addr, "removed from partition blocklist");
    }

    /// Clear all blocked addresses (for partition testing).
    pub async fn clear_blocklist(&self) {
        let mut state = self.state.write().await;
        state.partition_blocklist.clear();
        tracing::debug!("partition blocklist cleared");
    }

    /// Disconnect a peer by their socket address.
    ///
    /// Returns true if a peer was found and disconnected, false otherwise.
    pub async fn disconnect_addr(&self, addr: &SocketAddr) -> bool {
        let sender = {
            let state = self.state.read().await;
            if let Some(&peer_id) = state.connected_addrs.get(addr) {
                state.peer_senders.get(&peer_id).cloned()
            } else {
                None
            }
        };

        if let Some(sender) = sender {
            let _ = sender.send(PeerCommand::Disconnect).await;
            true
        } else {
            false
        }
    }

    /// Get list of connected socket addresses.
    pub async fn connected_addrs(&self) -> Vec<SocketAddr> {
        let state = self.state.read().await;
        state.connected_addrs.keys().cloned().collect()
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
