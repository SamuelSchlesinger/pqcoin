//! Peer connection management.
//!
//! This module handles individual peer connections, including:
//! - Message framing and parsing
//! - Handshake protocol
//! - Connection state tracking
//! - Send/receive operations

use crate::network::message::{Message, MessageError, Services, NETWORK_MAGIC, PROTOCOL_VERSION};
use std::net::SocketAddr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

/// Maximum message payload size (10 MB).
pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;

/// Header size in bytes.
pub const HEADER_SIZE: usize = 24;

/// Errors that can occur during peer operations.
#[derive(Debug, Error)]
pub enum PeerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("message error: {0}")]
    Message(#[from] MessageError),
    #[error("handshake failed: {0}")]
    HandshakeFailed(String),
    #[error("connection closed")]
    ConnectionClosed,
    #[error("message too large: {0} bytes")]
    MessageTooLarge(usize),
    #[error("timeout")]
    Timeout,
    #[error("peer misbehaving: {0}")]
    Misbehaving(String),
}

/// State of a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Connection established, awaiting handshake.
    Connecting,
    /// Handshake in progress.
    Handshaking,
    /// Fully connected and verified.
    Connected,
    /// Connection is being closed.
    Disconnecting,
    /// Connection has been closed.
    Disconnected,
}

/// Information about a connected peer.
#[derive(Debug)]
pub struct PeerInfo {
    /// Remote address.
    pub addr: SocketAddr,
    /// Protocol version reported by peer.
    pub version: u32,
    /// Services offered by peer.
    pub services: Services,
    /// Blockchain height reported by peer.
    pub height: u64,
    /// Whether we initiated the connection.
    pub outbound: bool,
    /// Time of last message received.
    pub last_seen: Instant,
    /// Misbehavior score (higher = worse).
    pub ban_score: u32,
    /// Whether peer wants transaction relay.
    pub relay: bool,
}

/// A peer connection handler.
pub struct Peer {
    /// Unique peer ID.
    id: u64,
    /// Remote socket address.
    addr: SocketAddr,
    /// TCP connection.
    stream: TcpStream,
    /// Connection state.
    state: PeerState,
    /// Peer info (populated after handshake).
    info: Option<PeerInfo>,
    /// Whether we initiated this connection.
    outbound: bool,
    /// Read buffer for incoming data (reserved for future streaming).
    #[allow(dead_code)]
    read_buf: Vec<u8>,
    /// Our listening address to advertise.
    local_addr: SocketAddr,
    /// Our blockchain height.
    local_height: u64,
    /// Our services.
    local_services: Services,
    /// Random nonce for self-connection detection.
    nonce: u64,
    /// User agent string.
    user_agent: String,
}

impl Peer {
    /// Create a new peer from an established TCP connection.
    pub fn new(
        id: u64,
        addr: SocketAddr,
        stream: TcpStream,
        outbound: bool,
        local_addr: SocketAddr,
        local_height: u64,
    ) -> Self {
        // Generate random nonce for self-connection detection
        let nonce = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
            ^ (id << 32);

        Self {
            id,
            addr,
            stream,
            state: PeerState::Connecting,
            info: None,
            outbound,
            read_buf: Vec::with_capacity(HEADER_SIZE),
            local_addr,
            local_height,
            local_services: Services::NODE_NETWORK,
            nonce,
            user_agent: "/pqcoin:0.1.0/".to_string(),
        }
    }

    /// Get our nonce (for self-connection detection).
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Get the peer's unique ID.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Get the peer's address.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Get the current connection state.
    pub fn state(&self) -> PeerState {
        self.state
    }

    /// Get peer info (if handshake completed).
    pub fn info(&self) -> Option<&PeerInfo> {
        self.info.as_ref()
    }

    /// Check if this is an outbound connection.
    pub fn is_outbound(&self) -> bool {
        self.outbound
    }

    /// Update the local blockchain height.
    pub fn set_local_height(&mut self, height: u64) {
        self.local_height = height;
    }

    /// Perform the handshake protocol.
    ///
    /// For outbound connections:
    /// 1. Send Version
    /// 2. Receive Version
    /// 3. Send Verack
    /// 4. Receive Verack
    ///
    /// For inbound connections:
    /// 1. Receive Version
    /// 2. Send Version
    /// 3. Receive Verack
    /// 4. Send Verack
    /// Build our version message.
    fn build_version_message(&self) -> Message {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Message::Version {
            version: PROTOCOL_VERSION,
            services: self.local_services,
            timestamp,
            height: self.local_height,
            nonce: self.nonce,
            addr: self.local_addr,
            user_agent: self.user_agent.clone(),
            relay: true,
        }
    }

    pub async fn handshake(&mut self) -> Result<(), PeerError> {
        self.state = PeerState::Handshaking;

        if self.outbound {
            // Send our version first
            self.send_message(&self.build_version_message()).await?;

            // Wait for their version
            let their_version = self.receive_message().await?;
            let (version, services, height, peer_nonce, relay) = match their_version {
                Message::Version { version, services, height, nonce, relay, .. } => {
                    (version, services, height, nonce, relay)
                }
                _ => {
                    return Err(PeerError::HandshakeFailed(
                        "expected Version message".to_string(),
                    ))
                }
            };

            // Check for self-connection
            if peer_nonce == self.nonce {
                return Err(PeerError::HandshakeFailed(
                    "self-connection detected".to_string(),
                ));
            }

            // Send verack
            self.send_message(&Message::Verack).await?;

            // Wait for verack
            let verack = self.receive_message().await?;
            if !matches!(verack, Message::Verack) {
                return Err(PeerError::HandshakeFailed(
                    "expected Verack message".to_string(),
                ));
            }

            self.info = Some(PeerInfo {
                addr: self.addr,
                version,
                services,
                height,
                outbound: true,
                last_seen: Instant::now(),
                ban_score: 0,
                relay,
            });
        } else {
            // Wait for their version first
            let their_version = self.receive_message().await?;
            let (version, services, height, peer_nonce, relay) = match their_version {
                Message::Version { version, services, height, nonce, relay, .. } => {
                    (version, services, height, nonce, relay)
                }
                _ => {
                    return Err(PeerError::HandshakeFailed(
                        "expected Version message".to_string(),
                    ))
                }
            };

            // Check for self-connection
            if peer_nonce == self.nonce {
                return Err(PeerError::HandshakeFailed(
                    "self-connection detected".to_string(),
                ));
            }

            // Send our version
            self.send_message(&self.build_version_message()).await?;

            // Wait for verack
            let verack = self.receive_message().await?;
            if !matches!(verack, Message::Verack) {
                return Err(PeerError::HandshakeFailed(
                    "expected Verack message".to_string(),
                ));
            }

            // Send verack
            self.send_message(&Message::Verack).await?;

            self.info = Some(PeerInfo {
                addr: self.addr,
                version,
                services,
                height,
                outbound: false,
                last_seen: Instant::now(),
                ban_score: 0,
                relay,
            });
        }

        self.state = PeerState::Connected;
        Ok(())
    }

    /// Send a message to this peer.
    pub async fn send_message(&mut self, msg: &Message) -> Result<(), PeerError> {
        let data = msg.serialize_with_header();
        self.stream.write_all(&data).await?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Receive a single message from this peer.
    pub async fn receive_message(&mut self) -> Result<Message, PeerError> {
        // Read header first
        let mut header = [0u8; HEADER_SIZE];
        self.stream.read_exact(&mut header).await?;

        // Verify magic
        if header[..4] != NETWORK_MAGIC {
            return Err(PeerError::Message(MessageError::InvalidMagic));
        }

        // Get payload length
        let length = u32::from_le_bytes([header[16], header[17], header[18], header[19]]) as usize;

        if length > MAX_MESSAGE_SIZE {
            return Err(PeerError::MessageTooLarge(length));
        }

        // Allocate once for both header and payload to avoid double allocation
        let mut full_message = vec![0u8; HEADER_SIZE + length];
        full_message[..HEADER_SIZE].copy_from_slice(&header);

        // Read payload directly into the combined buffer
        if length > 0 {
            self.stream.read_exact(&mut full_message[HEADER_SIZE..]).await?;
        }

        let msg = Message::deserialize_with_header(&full_message)?;

        // Update last seen time
        if let Some(ref mut info) = self.info {
            info.last_seen = Instant::now();
        }

        Ok(msg)
    }

    /// Close the connection gracefully.
    pub async fn disconnect(&mut self) {
        self.state = PeerState::Disconnecting;
        let _ = self.stream.shutdown().await;
        self.state = PeerState::Disconnected;
    }

    /// Add to the peer's ban score.
    pub fn add_ban_score(&mut self, score: u32, reason: &str) -> bool {
        if let Some(ref mut info) = self.info {
            info.ban_score = info.ban_score.saturating_add(score);
            tracing::warn!(
                peer = %self.addr,
                score = info.ban_score,
                reason = reason,
                "peer misbehavior"
            );
            info.ban_score >= 100
        } else {
            false
        }
    }
}

/// Handle for sending messages to a peer from other tasks.
#[derive(Clone)]
pub struct PeerHandle {
    /// Peer ID.
    pub id: u64,
    /// Peer address.
    pub addr: SocketAddr,
    /// Channel to send messages.
    tx: mpsc::Sender<Message>,
}

impl PeerHandle {
    /// Create a new peer handle.
    pub fn new(id: u64, addr: SocketAddr, tx: mpsc::Sender<Message>) -> Self {
        Self { id, addr, tx }
    }

    /// Send a message to this peer.
    pub async fn send(&self, msg: Message) -> Result<(), mpsc::error::SendError<Message>> {
        self.tx.send(msg).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_handshake() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let (stream, addr) = listener.accept().await.unwrap();
            let mut peer = Peer::new(1, addr, stream, false, server_addr, 100);
            peer.handshake().await.unwrap();
            assert_eq!(peer.state(), PeerState::Connected);
            assert!(peer.info().is_some());
            let info = peer.info().unwrap();
            assert_eq!(info.version, PROTOCOL_VERSION);
        });

        let client_handle = tokio::spawn(async move {
            let stream = TcpStream::connect(server_addr).await.unwrap();
            let mut peer = Peer::new(2, server_addr, stream, true, "127.0.0.1:0".parse().unwrap(), 50);
            peer.handshake().await.unwrap();
            assert_eq!(peer.state(), PeerState::Connected);
            assert!(peer.info().is_some());
            let info = peer.info().unwrap();
            assert_eq!(info.version, PROTOCOL_VERSION);
            assert_eq!(info.height, 100);
        });

        server_handle.await.unwrap();
        client_handle.await.unwrap();
    }
}
