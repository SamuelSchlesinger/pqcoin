//! Network events and errors.
//!
//! This module defines the event types produced by the network service
//! and the error types that can occur during network operations.

use crate::blockchain::{Block, Transaction};
use crate::network::peer::PeerError;
use crate::network::sync::SyncState;
use std::net::SocketAddr;
use thiserror::Error;

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
