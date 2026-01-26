//! P2P networking for pqcoin.
//!
//! This module implements a custom P2P networking layer for block and transaction
//! propagation. It uses TCP with length-prefixed framing for message transport.
//!
//! ## Architecture
//!
//! The networking layer follows an actor-like model:
//!
//! - [`NetworkService`]: Main coordinator managing peers and message routing
//! - [`Peer`]: Individual peer connection handler
//! - [`Message`]: Protocol messages for communication
//!
//! ## Protocol
//!
//! The protocol uses a simple message format:
//!
//! ```text
//! +---------+---------+--------+----------+---------+
//! | Magic   | Command | Length | Checksum | Payload |
//! | 4 bytes | 12 bytes| 4 bytes| 4 bytes  | variable|
//! +---------+---------+--------+----------+---------+
//! ```
//!
//! ## Handshake
//!
//! Connection establishment follows this sequence:
//!
//! 1. Initiator sends `Version` message
//! 2. Responder sends `Version` message
//! 3. Both sides send `Verack` to acknowledge
//! 4. Connection is established
//!
//! ## Example
//!
//! ```no_run
//! use pqcoin::network::{NetworkService, NetworkConfig};
//! use pqcoin::blockchain::{Blockchain, create_genesis_block, Address};
//! use pqcoin::crypto::ml_dsa_87;
//! use std::sync::Arc;
//! use tokio::sync::RwLock;
//!
//! #[tokio::main]
//! async fn main() {
//!     let (pk, _) = ml_dsa_87::keygen();
//!     let addr = Address::from_public_key(&pk);
//!     let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, addr);
//!     let blockchain = Blockchain::new(genesis, 2016, 600, 50_000_000, 210_000);
//!     let blockchain = Arc::new(RwLock::new(blockchain));
//!
//!     let config = NetworkConfig::default();
//!     let mut service = NetworkService::new(blockchain, config);
//!
//!     // Take the event receiver to handle network events
//!     let mut events = service.take_event_receiver().unwrap();
//!
//!     // Run the service (this blocks)
//!     // service.run().await.unwrap();
//! }
//! ```

mod message;
mod peer;
mod service;
mod sync;

pub use message::{
    InvItem, InvType, MAX_ADDR_COUNT, MAX_INV_COUNT, MAX_LOCATOR_COUNT, Message, MessageError,
    NETWORK_MAGIC, PROTOCOL_VERSION, Services, TimestampedAddr,
};
pub use peer::{HEADER_SIZE, MAX_MESSAGE_SIZE, Peer, PeerError, PeerHandle, PeerInfo, PeerState};
pub use service::{
    NetworkConfig, NetworkError, NetworkEvent, NetworkService, NetworkState, PeerCommand,
};
pub use sync::{SyncError, SyncManager, SyncState};

// Re-export constants from the centralized constants module
pub use crate::constants::{
    DEFAULT_PORT, MAX_BLOCKS_IN_FLIGHT, MAX_HEADERS_COUNT, MAX_OUTBOUND, MAX_PEERS,
    MAX_PENDING_HEADERS, PING_INTERVAL_SECS, PING_TIMEOUT_SECS,
};
