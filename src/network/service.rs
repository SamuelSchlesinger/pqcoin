//! Network service - main coordinator for P2P networking.
//!
//! The `NetworkService` manages:
//! - Listening for incoming connections
//! - Connecting to peers
//! - Message routing between peers and local handlers
//! - Block and transaction relay

use crate::blockchain::{Block, Blockchain, Transaction};
use crate::constants::{
    BAN_DURATION_SECS, BAN_SCORE_THRESHOLD, CONNECTION_RATE_LIMIT_SECS, DEFAULT_PORT,
    MAX_CONNECTIONS_PER_IP, MAX_OUTBOUND, MAX_PEERS, MAX_PER_SUBNET, PING_INTERVAL_SECS,
};
use crate::mempool::Mempool;
use crate::network::message::{InvItem, Message};
use crate::network::peer::{Peer, PeerError};
use crate::network::sync::{SyncManager, SyncState};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
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

/// Events produced by the network service.
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// A new peer has connected.
    PeerConnected { peer_id: u64, addr: SocketAddr },
    /// A peer has disconnected.
    PeerDisconnected { peer_id: u64, addr: SocketAddr },
    /// A new block has been received.
    NewBlock(Block),
    /// A new transaction has been received.
    NewTransaction(Transaction),
    /// Sync state has changed.
    SyncStateChanged(SyncState),
}

/// Errors from the network service.
#[derive(Debug, Error)]
pub enum NetworkError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("peer error: {0}")]
    Peer(#[from] PeerError),
    #[error("max peers reached")]
    MaxPeersReached,
    #[error("already connected to peer")]
    AlreadyConnected,
}

/// Message sent to a peer task.
#[derive(Debug)]
enum PeerCommand {
    /// Send a message to the peer.
    Send(Message),
    /// Disconnect the peer.
    Disconnect,
}

/// Message sent from a peer task to the service.
#[derive(Debug)]
enum PeerMessage {
    /// Handshake completed successfully.
    HandshakeComplete { peer_id: u64, height: u64 },
    /// A message was received from the peer.
    Received { peer_id: u64, message: Message },
    /// The peer disconnected.
    Disconnected { peer_id: u64, error: Option<String> },
}

/// Ban list for misbehaving peers.
///
/// Tracks banned IP addresses with expiration times.
struct BanList {
    /// Banned IPs with their ban expiry time.
    banned_ips: HashMap<IpAddr, Instant>,
}

impl BanList {
    fn new() -> Self {
        Self {
            banned_ips: HashMap::new(),
        }
    }

    /// Check if an IP is currently banned.
    fn is_banned(&self, ip: &IpAddr) -> bool {
        if let Some(expiry) = self.banned_ips.get(ip) {
            Instant::now() < *expiry
        } else {
            false
        }
    }

    /// Ban an IP for the configured duration.
    fn ban(&mut self, ip: IpAddr) {
        let expiry = Instant::now() + Duration::from_secs(BAN_DURATION_SECS);
        self.banned_ips.insert(ip, expiry);
        tracing::info!(ip = %ip, duration_secs = BAN_DURATION_SECS, "banned peer");
    }

    /// Clean up expired bans.
    fn cleanup(&mut self) {
        let now = Instant::now();
        self.banned_ips.retain(|_, expiry| now < *expiry);
    }
}

/// Rate limiter for incoming connections.
///
/// Prevents DoS attacks by limiting connection attempts per IP address.
struct RateLimiter {
    /// Last connection attempt time per IP.
    ip_last_connect: HashMap<IpAddr, Instant>,
    /// Current connection count per IP.
    ip_connection_count: HashMap<IpAddr, usize>,
}

impl RateLimiter {
    fn new() -> Self {
        Self {
            ip_last_connect: HashMap::new(),
            ip_connection_count: HashMap::new(),
        }
    }

    /// Check if a connection from this IP should be allowed.
    fn check_allowed(&mut self, ip: IpAddr) -> bool {
        let now = Instant::now();

        // Check rate limit - must wait at least CONNECTION_RATE_LIMIT_SECS between attempts
        if let Some(last) = self.ip_last_connect.get(&ip) {
            if now.duration_since(*last) < Duration::from_secs(CONNECTION_RATE_LIMIT_SECS) {
                return false;
            }
        }

        // Check connection count per IP
        let count = self.ip_connection_count.get(&ip).copied().unwrap_or(0);
        if count >= MAX_CONNECTIONS_PER_IP {
            return false;
        }

        self.ip_last_connect.insert(ip, now);
        true
    }

    /// Record that a connection was established from this IP.
    fn record_connection(&mut self, ip: IpAddr) {
        *self.ip_connection_count.entry(ip).or_insert(0) += 1;
    }

    /// Record that a connection was closed from this IP.
    fn record_disconnection(&mut self, ip: IpAddr) {
        if let Some(count) = self.ip_connection_count.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ip_connection_count.remove(&ip);
            }
        }
    }

    /// Clean up stale entries older than 1 hour.
    fn cleanup(&mut self) {
        let now = Instant::now();
        let stale_threshold = Duration::from_secs(3600); // 1 hour

        self.ip_last_connect.retain(|_, last| {
            now.duration_since(*last) < stale_threshold
        });
    }
}

/// Subnet limiter for eclipse attack protection.
///
/// Limits the number of connections from any single /16 subnet to prevent
/// an attacker from monopolizing connections with IPs from the same subnet.
struct SubnetLimiter {
    /// Connection count per /16 subnet prefix.
    subnet_counts: HashMap<[u8; 2], usize>,
}

impl SubnetLimiter {
    fn new() -> Self {
        Self {
            subnet_counts: HashMap::new(),
        }
    }

    /// Extract /16 prefix from IP address.
    ///
    /// For IPv4: returns first 2 bytes directly.
    /// For IPv6: returns first 2 bytes of the address (covers /16 equivalent).
    fn get_subnet_prefix(ip: &IpAddr) -> [u8; 2] {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                [octets[0], octets[1]]
            }
            IpAddr::V6(ipv6) => {
                let octets = ipv6.octets();
                [octets[0], octets[1]]
            }
        }
    }

    /// Check if we can accept another connection from this subnet.
    fn check_allowed(&self, ip: &IpAddr) -> bool {
        let prefix = Self::get_subnet_prefix(ip);
        let count = self.subnet_counts.get(&prefix).copied().unwrap_or(0);
        count < MAX_PER_SUBNET
    }

    /// Record a new connection from this subnet.
    fn record_connection(&mut self, ip: &IpAddr) {
        let prefix = Self::get_subnet_prefix(ip);
        *self.subnet_counts.entry(prefix).or_insert(0) += 1;
    }

    /// Record a disconnection from this subnet.
    fn record_disconnection(&mut self, ip: &IpAddr) {
        let prefix = Self::get_subnet_prefix(ip);
        if let Some(count) = self.subnet_counts.get_mut(&prefix) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.subnet_counts.remove(&prefix);
            }
        }
    }
}

/// Shared state for the network service.
struct NetworkState {
    /// Connected peers.
    peers: HashMap<u64, PeerInfo>,
    /// Addresses we're connected to (to avoid duplicates).
    connected_addrs: HashMap<SocketAddr, u64>,
    /// Channels to send commands to peer tasks.
    peer_senders: HashMap<u64, mpsc::Sender<PeerCommand>>,
    /// Known peer addresses.
    known_addrs: Vec<SocketAddr>,
    /// Number of outbound connections.
    outbound_count: usize,
    /// Rate limiter for incoming connections.
    rate_limiter: RateLimiter,
    /// Ban list for misbehaving peers.
    ban_list: BanList,
    /// Subnet limiter for eclipse attack protection.
    subnet_limiter: SubnetLimiter,
}

#[derive(Clone)]
struct PeerInfo {
    addr: SocketAddr,
    height: u64,
    outbound: bool,
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
            state: Arc::new(RwLock::new(NetworkState {
                peers: HashMap::new(),
                connected_addrs: HashMap::new(),
                peer_senders: HashMap::new(),
                known_addrs: Vec::new(),
                outbound_count: 0,
                rate_limiter: RateLimiter::new(),
                ban_list: BanList::new(),
                subnet_limiter: SubnetLimiter::new(),
            })),
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
                    self.handle_peer_message(msg).await;
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

    /// Handle a message from a peer task.
    async fn handle_peer_message(&self, msg: PeerMessage) {
        match msg {
            PeerMessage::HandshakeComplete { peer_id, height } => {
                // Update peer's height in our state
                let mut state = self.state.write().await;
                if let Some(info) = state.peers.get_mut(&peer_id) {
                    info.height = height;
                    tracing::debug!(peer_id = peer_id, height = height, "updated peer height");
                }
            }
            PeerMessage::Received { peer_id, message } => {
                self.handle_message(peer_id, message).await;
            }
            PeerMessage::Disconnected { peer_id, error } => {
                self.handle_disconnect(peer_id, error).await;
            }
        }
    }

    /// Handle a message from a peer.
    async fn handle_message(&self, peer_id: u64, message: Message) {
        match message {
            Message::Ping(nonce) => {
                tracing::debug!(peer_id = peer_id, nonce = nonce, "received ping, sending pong");
                self.send_to_peer(peer_id, Message::Pong(nonce)).await;
            }

            Message::Pong(nonce) => {
                tracing::debug!(peer_id = peer_id, nonce = nonce, "received pong");
            }

            Message::GetAddr => {
                let state = self.state.read().await;
                let addrs: Vec<SocketAddr> = state
                    .known_addrs
                    .iter()
                    .take(1000)
                    .cloned()
                    .collect();
                drop(state);
                self.send_to_peer(peer_id, Message::Addr { addrs }).await;
            }

            Message::Addr { addrs } => {
                let mut state = self.state.write().await;
                for addr in addrs {
                    if !state.known_addrs.contains(&addr) {
                        state.known_addrs.push(addr);
                    }
                }
            }

            Message::AddrV2 { addrs } => {
                // Handle timestamped addresses - filter out stale ones
                let current_time = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                let mut state = self.state.write().await;
                for taddr in addrs {
                    if !taddr.is_stale(current_time) && !state.known_addrs.contains(&taddr.addr) {
                        state.known_addrs.push(taddr.addr);
                    }
                }
            }

            Message::SendHeaders => {
                // Peer requests block announcements via headers
                // TODO: Track this preference per-peer and use Headers instead of Inv
                tracing::debug!(peer_id = peer_id, "peer requested SendHeaders mode");
            }

            Message::Inv { items } => {
                let mempool = self.mempool.read().await;
                let mut sync = self.sync.write().await;
                if let Some(response) = sync.process_inv(&items, |hash| mempool.contains(hash)).await {
                    drop(sync);
                    drop(mempool);
                    self.send_to_peer(peer_id, response).await;
                }
            }

            Message::GetData { items } => {
                let mempool = self.mempool.read().await;
                let sync = self.sync.read().await;
                let responses = sync.respond_to_get_data(&items, |hash| mempool.get(hash).cloned()).await;
                drop(sync);
                drop(mempool);
                for response in responses {
                    self.send_to_peer(peer_id, response).await;
                }
            }

            Message::GetHeaders { locator, stop } => {
                let sync = self.sync.read().await;
                let response = sync.respond_to_get_headers(&locator, &stop).await;
                drop(sync);
                self.send_to_peer(peer_id, response).await;
            }

            Message::Headers { headers } => {
                let mut sync = self.sync.write().await;
                match sync.process_headers(peer_id, headers).await {
                    Ok(responses) => {
                        drop(sync);
                        for response in responses {
                            self.send_to_peer(peer_id, response).await;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(peer_id = peer_id, error = %e, "header processing failed");
                    }
                }
            }

            Message::Block(block) => {
                // First, process block with sync lock only
                let block_hash = block.hash();
                let added = {
                    let mut sync = self.sync.write().await;
                    sync.process_block(peer_id, block.clone()).await
                };

                match added {
                    Ok(true) => {
                        // Block was added, now update mempool separately
                        {
                            let mut mempool = self.mempool.write().await;
                            mempool.remove_confirmed(&block.transactions);
                            mempool.remove_conflicts(&block.transactions);
                        }

                        // Events and broadcast without holding locks
                        let _ = self.event_tx.send(NetworkEvent::NewBlock(block.clone())).await;
                        self.broadcast_except(peer_id, Message::Inv {
                            items: vec![InvItem::block(block_hash)],
                        }).await;
                    }
                    Ok(false) => {
                        // Orphan block, already stored
                    }
                    Err(e) => {
                        tracing::warn!(peer_id = peer_id, error = %e, "block processing failed");
                    }
                }
            }

            Message::Tx(tx) => {
                // Validate and add to mempool
                let txid = tx.txid();
                let blockchain = self.blockchain.read().await;
                let current_height = blockchain.height();
                let mut mempool = self.mempool.write().await;

                match mempool.add(tx.clone(), &blockchain, current_height) {
                    Ok(true) => {
                        drop(mempool);
                        drop(blockchain);

                        tracing::debug!(
                            txid = %txid.to_hex()[..16],
                            "added transaction to mempool"
                        );

                        let _ = self
                            .event_tx
                            .send(NetworkEvent::NewTransaction(tx.clone()))
                            .await;

                        // Relay to other peers
                        self.broadcast_except(peer_id, Message::Inv {
                            items: vec![InvItem::tx(txid)],
                        }).await;
                    }
                    Ok(false) => {
                        // Already in mempool, ignore
                        tracing::trace!(txid = %txid.to_hex()[..16], "transaction already in mempool");
                    }
                    Err(e) => {
                        tracing::debug!(
                            txid = %txid.to_hex()[..16],
                            error = %e,
                            "rejected transaction"
                        );
                    }
                }
            }

            Message::Reject { message, reason } => {
                tracing::warn!(
                    peer_id = peer_id,
                    message = message,
                    reason = reason,
                    "peer rejected message"
                );
            }

            Message::Version { .. } | Message::Verack => {
                // Should not receive these after handshake
                tracing::warn!(peer_id = peer_id, "unexpected handshake message");
            }
        }
    }

    /// Handle a peer disconnection.
    async fn handle_disconnect(&self, peer_id: u64, error: Option<String>) {
        let addr = {
            let mut state = self.state.write().await;
            let info = state.peers.remove(&peer_id);
            state.peer_senders.remove(&peer_id);

            if let Some(info) = &info {
                state.connected_addrs.remove(&info.addr);
                if info.outbound {
                    state.outbound_count = state.outbound_count.saturating_sub(1);
                }
                // Update rate limiter and subnet limiter connection counts
                state.rate_limiter.record_disconnection(info.addr.ip());
                state.subnet_limiter.record_disconnection(&info.addr.ip());
            }

            info.map(|i| i.addr)
        };

        if let Some(addr) = addr {
            tracing::info!(
                peer_id = peer_id,
                addr = %addr,
                error = error.as_deref().unwrap_or("none"),
                "peer disconnected"
            );

            let _ = self
                .event_tx
                .send(NetworkEvent::PeerDisconnected { peer_id, addr })
                .await;

            // Notify sync manager
            let mut sync = self.sync.write().await;
            sync.handle_peer_disconnected(peer_id);
        }
    }

    /// Send a message to a specific peer.
    async fn send_to_peer(&self, peer_id: u64, message: Message) {
        let state = self.state.read().await;
        if let Some(sender) = state.peer_senders.get(&peer_id) {
            let _ = sender.send(PeerCommand::Send(message)).await;
        }
    }

    /// Broadcast a message to all peers except one.
    async fn broadcast_except(&self, except_peer_id: u64, message: Message) {
        let state = self.state.read().await;
        for (&peer_id, sender) in &state.peer_senders {
            if peer_id != except_peer_id {
                let _ = sender.send(PeerCommand::Send(message.clone())).await;
            }
        }
    }

    /// Broadcast a message to all peers.
    pub async fn broadcast(&self, message: Message) {
        let state = self.state.read().await;
        for sender in state.peer_senders.values() {
            let _ = sender.send(PeerCommand::Send(message.clone())).await;
        }
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
        let _ = self.event_tx.send(NetworkEvent::NewBlock(block.clone())).await;

        // Broadcast inventory to all peers
        self.broadcast(Message::Inv {
            items: vec![InvItem::block(hash)],
        })
        .await;
    }

    /// Submit a local transaction to the mempool and broadcast it.
    pub async fn submit_transaction(&self, tx: Transaction) -> Result<(), crate::mempool::MempoolError> {
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
    pub async fn submit_block(&self, block: Block) -> Result<(), crate::blockchain::BlockchainError> {
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
        let _ = self.event_tx.send(NetworkEvent::NewBlock(block.clone())).await;

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
    use crate::blockchain::{create_genesis_block, Address};
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
