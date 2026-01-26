//! Network state management.
//!
//! This module contains the shared state for the network service,
//! including peer tracking, connection management, and internal messaging.

use super::addrman::AddressManager;
use crate::network::message::Message;
use crate::network::service::defense::{BanList, PartitionBlocklist, RateLimiter, SubnetLimiter};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;
use tokio::sync::mpsc;

/// Message sent to a peer task.
#[derive(Debug)]
pub enum PeerCommand {
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
pub struct NetworkState {
    /// Connected peers.
    pub(crate) peers: HashMap<u64, PeerInfo>,
    /// Addresses we're connected to (to avoid duplicates).
    pub(crate) connected_addrs: HashMap<SocketAddr, u64>,
    /// Channels to send commands to peer tasks.
    pub(crate) peer_senders: HashMap<u64, mpsc::Sender<PeerCommand>>,
    /// Address manager for peer discovery.
    pub(crate) addr_manager: AddressManager,
    /// Peers awaiting GetAddr (peer_id -> handshake completion time).
    pub(crate) pending_getaddr: HashMap<u64, Instant>,
    /// Number of outbound connections.
    pub(crate) outbound_count: usize,
    /// Rate limiter for incoming connections.
    pub(crate) rate_limiter: RateLimiter,
    /// Ban list for misbehaving peers.
    pub(crate) ban_list: BanList,
    /// Subnet limiter for eclipse attack protection.
    pub(crate) subnet_limiter: SubnetLimiter,
    /// Partition blocklist for testing (addresses blocked by test partitions).
    pub partition_blocklist: PartitionBlocklist,
    /// Last GetAddr response time per peer (for rate limiting).
    pub(crate) last_getaddr_response: HashMap<u64, Instant>,
}

impl NetworkState {
    /// Create a new NetworkState with default values.
    pub(crate) fn new() -> Self {
        Self {
            peers: HashMap::new(),
            connected_addrs: HashMap::new(),
            peer_senders: HashMap::new(),
            addr_manager: AddressManager::new(),
            pending_getaddr: HashMap::new(),
            outbound_count: 0,
            rate_limiter: RateLimiter::new(),
            ban_list: BanList::new(),
            subnet_limiter: SubnetLimiter::new(),
            partition_blocklist: PartitionBlocklist::new(),
            last_getaddr_response: HashMap::new(),
        }
    }

    /// Get the number of connected peers.
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    /// Get list of connected socket addresses.
    pub fn connected_addrs(&self) -> Vec<SocketAddr> {
        self.connected_addrs.keys().cloned().collect()
    }

    /// Get the peer sender for a given address, if connected.
    ///
    /// This is used for testing to disconnect peers by address.
    pub fn get_peer_sender(&self, addr: &SocketAddr) -> Option<mpsc::Sender<PeerCommand>> {
        self.connected_addrs
            .get(addr)
            .and_then(|&peer_id| self.peer_senders.get(&peer_id).cloned())
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
