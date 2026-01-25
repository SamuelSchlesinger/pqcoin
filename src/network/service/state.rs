//! Network state management.
//!
//! This module contains the shared state for the network service,
//! including peer tracking, connection management, and internal messaging.

use crate::network::message::Message;
use crate::network::service::defense::{BanList, RateLimiter, SubnetLimiter};
use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::sync::mpsc;

/// Message sent to a peer task.
#[derive(Debug)]
pub(crate) enum PeerCommand {
    /// Send a message to the peer.
    Send(Message),
    /// Disconnect the peer.
    Disconnect,
}

/// Message sent from a peer task to the service.
#[derive(Debug)]
pub(crate) enum PeerMessage {
    /// Handshake completed successfully.
    HandshakeComplete { peer_id: u64, height: u64 },
    /// A message was received from the peer.
    Received { peer_id: u64, message: Message },
    /// The peer disconnected.
    Disconnected { peer_id: u64, error: Option<String> },
}

/// Shared state for the network service.
pub(crate) struct NetworkState {
    /// Connected peers.
    pub(crate) peers: HashMap<u64, PeerInfo>,
    /// Addresses we're connected to (to avoid duplicates).
    pub(crate) connected_addrs: HashMap<SocketAddr, u64>,
    /// Channels to send commands to peer tasks.
    pub(crate) peer_senders: HashMap<u64, mpsc::Sender<PeerCommand>>,
    /// Known peer addresses.
    pub(crate) known_addrs: Vec<SocketAddr>,
    /// Number of outbound connections.
    pub(crate) outbound_count: usize,
    /// Rate limiter for incoming connections.
    pub(crate) rate_limiter: RateLimiter,
    /// Ban list for misbehaving peers.
    pub(crate) ban_list: BanList,
    /// Subnet limiter for eclipse attack protection.
    pub(crate) subnet_limiter: SubnetLimiter,
}

impl NetworkState {
    /// Create a new NetworkState with default values.
    pub(crate) fn new() -> Self {
        Self {
            peers: HashMap::new(),
            connected_addrs: HashMap::new(),
            peer_senders: HashMap::new(),
            known_addrs: Vec::new(),
            outbound_count: 0,
            rate_limiter: RateLimiter::new(),
            ban_list: BanList::new(),
            subnet_limiter: SubnetLimiter::new(),
        }
    }
}

/// Information about a connected peer.
#[derive(Clone)]
pub(crate) struct PeerInfo {
    /// The peer's socket address.
    pub(crate) addr: SocketAddr,
    /// The peer's reported chain height.
    pub(crate) height: u64,
    /// Whether this is an outbound connection.
    pub(crate) outbound: bool,
}
