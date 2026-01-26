//! Network protocol message types.
//!
//! This module defines the P2P protocol messages for pqcoin network communication.
//! Messages use a simple length-prefixed framing format.
//!
//! # Security: Size Limits
//!
//! All variable-length fields have explicit size limits to prevent memory exhaustion:
//!
//! | Constant | Limit | Purpose |
//! |----------|-------|---------|
//! | `MAX_ADDR_COUNT` | 1,000 | Addresses per Addr/AddrV2 message |
//! | `MAX_INV_COUNT` | 50,000 | Items per Inv/GetData message |
//! | `MAX_LOCATOR_COUNT` | 101 | Block locators per GetHeaders |
//! | `MAX_HEADERS_COUNT` | 2,000 | Headers per Headers message |
//! | `MAX_STRING_SIZE` | 1 MB | Variable-length strings (user agent, etc.) |
//!
//! These limits are enforced during deserialization, before memory allocation.

use crate::blockchain::{
    Block, BlockHeader, Deserialize, DeserializeError, Serialize, Transaction,
};
use crate::constants::MAX_HEADERS_COUNT;
use crate::crypto::Hash;
use std::net::SocketAddr;
use thiserror::Error;

/// Network protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Magic bytes to identify pqcoin network messages ("PQCN").
pub const NETWORK_MAGIC: [u8; 4] = [0x50, 0x51, 0x43, 0x4E];

// ============================================================================
// Service Flags (similar to Bitcoin's service bits)
// ============================================================================

bitflags::bitflags! {
    /// Service flags advertised by nodes.
    ///
    /// These flags indicate what services a node can provide to the network.
    /// Similar to Bitcoin's service bits (BIP 111, etc.).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct Services: u64 {
        /// Node can serve full blocks (not pruned).
        const NODE_NETWORK = 1 << 0;
        /// Node supports bloom filtering (for light clients).
        const NODE_BLOOM = 1 << 1;
        /// Node can serve historical blocks (archival node).
        const NODE_NETWORK_LIMITED = 1 << 10;
    }
}

impl Default for Services {
    fn default() -> Self {
        Services::NODE_NETWORK
    }
}

// ============================================================================
// Timestamped Address for peer discovery
// ============================================================================

/// A peer address with timestamp for freshness tracking.
///
/// Addresses older than 3 hours are considered stale and deprioritized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampedAddr {
    /// Unix timestamp when this address was last seen active.
    pub timestamp: u64,
    /// Services offered by this peer.
    pub services: Services,
    /// Socket address.
    pub addr: SocketAddr,
}

impl TimestampedAddr {
    /// Maximum age (in seconds) before an address is considered stale.
    pub const MAX_AGE: u64 = 3 * 60 * 60; // 3 hours

    /// Check if this address is stale.
    pub fn is_stale(&self, current_time: u64) -> bool {
        current_time.saturating_sub(self.timestamp) > Self::MAX_AGE
    }
}

/// Maximum number of addresses in an Addr message.
pub const MAX_ADDR_COUNT: usize = 1000;

/// Maximum number of inventory items in Inv/GetData messages.
pub const MAX_INV_COUNT: usize = 50000;

/// Maximum number of block locator hashes.
pub const MAX_LOCATOR_COUNT: usize = 101;

/// Maximum length for variable-length strings in messages (1 MB).
/// This provides explicit bounds for defense in depth, though strings
/// are also bounded by the overall message size limit.
pub const MAX_STRING_SIZE: usize = 1024 * 1024;

/// Errors that can occur during message handling.
#[derive(Debug, Error)]
pub enum MessageError {
    #[error("invalid magic bytes")]
    InvalidMagic,
    #[error("unknown command: {0}")]
    UnknownCommand(String),
    #[error("payload too large: {0} bytes")]
    PayloadTooLarge(u32),
    #[error("checksum mismatch")]
    ChecksumMismatch,
    #[error("deserialization error: {0}")]
    DeserializeError(#[from] DeserializeError),
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Type of inventory item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InvType {
    /// Transaction inventory.
    Tx = 1,
    /// Block inventory.
    Block = 2,
}

impl Serialize for InvType {
    fn serialize(&self, buf: &mut Vec<u8>) {
        buf.push(*self as u8);
    }
}

impl Deserialize for InvType {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        if data.is_empty() {
            return Err(DeserializeError::UnexpectedEof);
        }
        let inv_type = match data[0] {
            1 => InvType::Tx,
            2 => InvType::Block,
            n => {
                return Err(DeserializeError::InvalidData(format!(
                    "unknown inv type: {n}"
                )));
            }
        };
        Ok((inv_type, &data[1..]))
    }
}

/// An inventory item (reference to a block or transaction).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvItem {
    /// Type of the inventory item.
    pub inv_type: InvType,
    /// Hash of the item.
    pub hash: Hash,
}

impl InvItem {
    /// Create a new inventory item for a transaction.
    pub fn tx(hash: Hash) -> Self {
        Self {
            inv_type: InvType::Tx,
            hash,
        }
    }

    /// Create a new inventory item for a block.
    pub fn block(hash: Hash) -> Self {
        Self {
            inv_type: InvType::Block,
            hash,
        }
    }
}

impl Serialize for InvItem {
    fn serialize(&self, buf: &mut Vec<u8>) {
        self.inv_type.serialize(buf);
        buf.extend_from_slice(self.hash.as_bytes());
    }
}

impl Deserialize for InvItem {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (inv_type, data) = InvType::deserialize(data)?;
        if data.len() < 64 {
            return Err(DeserializeError::UnexpectedEof);
        }
        let mut hash_bytes = [0u8; 64];
        hash_bytes.copy_from_slice(&data[..64]);
        Ok((
            InvItem {
                inv_type,
                hash: Hash::from_bytes(hash_bytes),
            },
            &data[64..],
        ))
    }
}

/// P2P protocol messages.
#[derive(Debug, Clone)]
pub enum Message {
    /// Version handshake: exchange version and capabilities.
    ///
    /// Enhanced compared to Bitcoin's version message to include:
    /// - Nonce for self-connection detection
    /// - Services bitmap for capability advertisement
    Version {
        /// Protocol version.
        version: u32,
        /// Services offered by this node.
        services: Services,
        /// Unix timestamp.
        timestamp: u64,
        /// Current blockchain height.
        height: u64,
        /// Random nonce to detect self-connections.
        nonce: u64,
        /// Our listening address.
        addr: SocketAddr,
        /// User agent string (e.g., "/pqcoin:0.1.0/").
        user_agent: String,
        /// Whether we want to receive transaction relay.
        relay: bool,
    },
    /// Acknowledge version handshake.
    Verack,
    /// Request peer's known peers.
    GetAddr,
    /// Response with known peer addresses (simple format).
    Addr {
        /// List of known peer addresses.
        addrs: Vec<SocketAddr>,
    },
    /// Response with timestamped peer addresses (enhanced format).
    ///
    /// Similar to Bitcoin's addrv2, includes timestamps for freshness.
    AddrV2 {
        /// List of timestamped peer addresses.
        addrs: Vec<TimestampedAddr>,
    },
    /// Request to receive block announcements via headers instead of inv.
    ///
    /// After receiving this, the peer should announce new blocks by sending
    /// a Headers message instead of an Inv message.
    SendHeaders,
    /// Advertise inventory (blocks/txs we have).
    Inv {
        /// Inventory items.
        items: Vec<InvItem>,
    },
    /// Request specific items by hash.
    GetData {
        /// Items to request.
        items: Vec<InvItem>,
    },
    /// Request block headers starting from locator hashes.
    GetHeaders {
        /// Block locator hashes (from tip back to genesis).
        locator: Vec<Hash>,
        /// Hash to stop at (zero hash means no limit).
        stop: Hash,
    },
    /// Response with block headers.
    Headers {
        /// Block headers.
        headers: Vec<BlockHeader>,
    },
    /// Full block data.
    Block(Block),
    /// Transaction data.
    Tx(Transaction),
    /// Ping for keepalive.
    Ping(u64),
    /// Pong response to ping.
    Pong(u64),
    /// Reject message (for protocol violations).
    Reject {
        /// The rejected message command.
        message: String,
        /// Rejection reason.
        reason: String,
    },
}

impl Message {
    /// Get the command name for this message (used in wire format).
    pub fn command(&self) -> &'static str {
        match self {
            Message::Version { .. } => "version",
            Message::Verack => "verack",
            Message::GetAddr => "getaddr",
            Message::Addr { .. } => "addr",
            Message::AddrV2 { .. } => "addrv2",
            Message::SendHeaders => "sendheaders",
            Message::Inv { .. } => "inv",
            Message::GetData { .. } => "getdata",
            Message::GetHeaders { .. } => "getheaders",
            Message::Headers { .. } => "headers",
            Message::Block(_) => "block",
            Message::Tx(_) => "tx",
            Message::Ping(_) => "ping",
            Message::Pong(_) => "pong",
            Message::Reject { .. } => "reject",
        }
    }

    /// Serialize the message payload (excluding header).
    fn serialize_payload(&self, buf: &mut Vec<u8>) {
        match self {
            Message::Version {
                version,
                services,
                timestamp,
                height,
                nonce,
                addr,
                user_agent,
                relay,
            } => {
                buf.extend_from_slice(&version.to_le_bytes());
                buf.extend_from_slice(&services.bits().to_le_bytes());
                buf.extend_from_slice(&timestamp.to_le_bytes());
                buf.extend_from_slice(&height.to_le_bytes());
                buf.extend_from_slice(&nonce.to_le_bytes());
                serialize_socket_addr(buf, addr);
                write_var_string(buf, user_agent);
                buf.push(if *relay { 1 } else { 0 });
            }
            Message::Verack => {}
            Message::GetAddr => {}
            Message::SendHeaders => {}
            Message::Addr { addrs } => {
                write_var_int(buf, addrs.len() as u64);
                for addr in addrs {
                    serialize_socket_addr(buf, addr);
                }
            }
            Message::AddrV2 { addrs } => {
                write_var_int(buf, addrs.len() as u64);
                for taddr in addrs {
                    buf.extend_from_slice(&taddr.timestamp.to_le_bytes());
                    buf.extend_from_slice(&taddr.services.bits().to_le_bytes());
                    serialize_socket_addr(buf, &taddr.addr);
                }
            }
            Message::Inv { items } => {
                write_var_int(buf, items.len() as u64);
                for item in items {
                    item.serialize(buf);
                }
            }
            Message::GetData { items } => {
                write_var_int(buf, items.len() as u64);
                for item in items {
                    item.serialize(buf);
                }
            }
            Message::GetHeaders { locator, stop } => {
                write_var_int(buf, locator.len() as u64);
                for hash in locator {
                    buf.extend_from_slice(hash.as_bytes());
                }
                buf.extend_from_slice(stop.as_bytes());
            }
            Message::Headers { headers } => {
                write_var_int(buf, headers.len() as u64);
                for header in headers {
                    header.serialize(buf);
                }
            }
            Message::Block(block) => {
                block.serialize(buf);
            }
            Message::Tx(tx) => {
                tx.serialize(buf);
            }
            Message::Ping(nonce) => {
                buf.extend_from_slice(&nonce.to_le_bytes());
            }
            Message::Pong(nonce) => {
                buf.extend_from_slice(&nonce.to_le_bytes());
            }
            Message::Reject { message, reason } => {
                write_var_string(buf, message);
                write_var_string(buf, reason);
            }
        }
    }

    /// Serialize the complete message with header.
    ///
    /// Wire format:
    /// - magic: 4 bytes
    /// - command: 12 bytes (null-padded)
    /// - length: 4 bytes (little-endian)
    /// - checksum: 4 bytes (first 4 bytes of double SHA3-512)
    /// - payload: variable
    pub fn serialize_with_header(&self) -> Vec<u8> {
        let mut payload = Vec::new();
        self.serialize_payload(&mut payload);

        let checksum = compute_checksum(&payload);

        let mut buf = Vec::with_capacity(24 + payload.len());

        // Magic
        buf.extend_from_slice(&NETWORK_MAGIC);

        // Command (12 bytes, null-padded)
        let cmd = self.command();
        let cmd_bytes = cmd.as_bytes();
        buf.extend_from_slice(cmd_bytes);
        buf.resize(buf.len() + (12 - cmd_bytes.len()), 0);

        // Length
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());

        // Checksum
        buf.extend_from_slice(&checksum);

        // Payload
        buf.extend_from_slice(&payload);

        buf
    }

    /// Deserialize a message from a complete frame (header + payload).
    pub fn deserialize_with_header(data: &[u8]) -> Result<Self, MessageError> {
        if data.len() < 24 {
            return Err(MessageError::DeserializeError(
                DeserializeError::UnexpectedEof,
            ));
        }

        // Verify magic
        if data[..4] != NETWORK_MAGIC {
            return Err(MessageError::InvalidMagic);
        }

        // Extract command
        let cmd_bytes = &data[4..16];
        let cmd_end = cmd_bytes.iter().position(|&b| b == 0).unwrap_or(12);
        let command = std::str::from_utf8(&cmd_bytes[..cmd_end])
            .map_err(|_| MessageError::UnknownCommand("invalid utf8".to_string()))?;

        // Extract length
        let length = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);

        // Extract checksum
        let expected_checksum = &data[20..24];

        // Verify we have the full payload
        if data.len() < 24 + length as usize {
            return Err(MessageError::DeserializeError(
                DeserializeError::UnexpectedEof,
            ));
        }

        let payload = &data[24..24 + length as usize];

        // Verify checksum
        let actual_checksum = compute_checksum(payload);
        if actual_checksum != expected_checksum {
            return Err(MessageError::ChecksumMismatch);
        }

        // Deserialize payload
        Self::deserialize_payload(command, payload)
    }

    /// Deserialize a message payload given its command.
    fn deserialize_payload(command: &str, data: &[u8]) -> Result<Self, MessageError> {
        match command {
            "version" => {
                // version(4) + services(8) + timestamp(8) + height(8) + nonce(8) = 36 minimum
                if data.len() < 36 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::UnexpectedEof,
                    ));
                }
                let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                let services_bits = u64::from_le_bytes([
                    data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
                ]);
                let services = Services::from_bits_truncate(services_bits);
                let timestamp = u64::from_le_bytes([
                    data[12], data[13], data[14], data[15], data[16], data[17], data[18], data[19],
                ]);
                let height = u64::from_le_bytes([
                    data[20], data[21], data[22], data[23], data[24], data[25], data[26], data[27],
                ]);
                let nonce = u64::from_le_bytes([
                    data[28], data[29], data[30], data[31], data[32], data[33], data[34], data[35],
                ]);
                let (addr, rest) = deserialize_socket_addr(&data[36..])?;
                let (user_agent, rest) = read_var_string(rest)?;
                let relay = if rest.is_empty() { true } else { rest[0] != 0 };
                Ok(Message::Version {
                    version,
                    services,
                    timestamp,
                    height,
                    nonce,
                    addr,
                    user_agent,
                    relay,
                })
            }
            "verack" => Ok(Message::Verack),
            "getaddr" => Ok(Message::GetAddr),
            "sendheaders" => Ok(Message::SendHeaders),
            "addr" => {
                let (count, rest) = read_var_int(data)?;
                if count > MAX_ADDR_COUNT as u64 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::LengthOverflow,
                    ));
                }
                let mut addrs = Vec::with_capacity(count as usize);
                let mut data = rest;
                for _ in 0..count {
                    let (addr, rest) = deserialize_socket_addr(data)?;
                    addrs.push(addr);
                    data = rest;
                }
                Ok(Message::Addr { addrs })
            }
            "addrv2" => {
                let (count, rest) = read_var_int(data)?;
                if count > MAX_ADDR_COUNT as u64 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::LengthOverflow,
                    ));
                }
                let mut addrs = Vec::with_capacity(count as usize);
                let mut data = rest;
                for _ in 0..count {
                    if data.len() < 16 {
                        return Err(MessageError::DeserializeError(
                            DeserializeError::UnexpectedEof,
                        ));
                    }
                    let timestamp = u64::from_le_bytes([
                        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                    ]);
                    let services_bits = u64::from_le_bytes([
                        data[8], data[9], data[10], data[11], data[12], data[13], data[14],
                        data[15],
                    ]);
                    let services = Services::from_bits_truncate(services_bits);
                    let (addr, rest) = deserialize_socket_addr(&data[16..])?;
                    addrs.push(TimestampedAddr {
                        timestamp,
                        services,
                        addr,
                    });
                    data = rest;
                }
                Ok(Message::AddrV2 { addrs })
            }
            "inv" => {
                let (count, rest) = read_var_int(data)?;
                if count > MAX_INV_COUNT as u64 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::LengthOverflow,
                    ));
                }
                // Use HashSet to deduplicate items and prevent memory exhaustion
                // from receiving many identical hashes
                let mut seen = std::collections::HashSet::with_capacity(count as usize);
                let mut items = Vec::with_capacity(count as usize);
                let mut data = rest;
                for _ in 0..count {
                    let (item, rest) = InvItem::deserialize(data)?;
                    // Only add if not already seen (dedup by hash and type)
                    if seen.insert((item.inv_type as u8, item.hash)) {
                        items.push(item);
                    }
                    data = rest;
                }
                Ok(Message::Inv { items })
            }
            "getdata" => {
                let (count, rest) = read_var_int(data)?;
                if count > MAX_INV_COUNT as u64 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::LengthOverflow,
                    ));
                }
                // Use HashSet to deduplicate items
                let mut seen = std::collections::HashSet::with_capacity(count as usize);
                let mut items = Vec::with_capacity(count as usize);
                let mut data = rest;
                for _ in 0..count {
                    let (item, rest) = InvItem::deserialize(data)?;
                    // Only add if not already seen (dedup by hash and type)
                    if seen.insert((item.inv_type as u8, item.hash)) {
                        items.push(item);
                    }
                    data = rest;
                }
                Ok(Message::GetData { items })
            }
            "getheaders" => {
                let (count, rest) = read_var_int(data)?;
                if count > MAX_LOCATOR_COUNT as u64 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::LengthOverflow,
                    ));
                }
                let mut locator = Vec::with_capacity(count as usize);
                let mut data = rest;
                for _ in 0..count {
                    if data.len() < 64 {
                        return Err(MessageError::DeserializeError(
                            DeserializeError::UnexpectedEof,
                        ));
                    }
                    let mut hash_bytes = [0u8; 64];
                    hash_bytes.copy_from_slice(&data[..64]);
                    locator.push(Hash::from_bytes(hash_bytes));
                    data = &data[64..];
                }
                if data.len() < 64 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::UnexpectedEof,
                    ));
                }
                let mut stop_bytes = [0u8; 64];
                stop_bytes.copy_from_slice(&data[..64]);
                let stop = Hash::from_bytes(stop_bytes);
                Ok(Message::GetHeaders { locator, stop })
            }
            "headers" => {
                let (count, rest) = read_var_int(data)?;
                if count > MAX_HEADERS_COUNT as u64 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::LengthOverflow,
                    ));
                }
                let mut headers = Vec::with_capacity(count as usize);
                let mut data = rest;
                for _ in 0..count {
                    let (header, rest) = BlockHeader::deserialize(data)?;
                    headers.push(header);
                    data = rest;
                }
                Ok(Message::Headers { headers })
            }
            "block" => {
                let (block, _) = Block::deserialize(data)?;
                Ok(Message::Block(block))
            }
            "tx" => {
                let (tx, _) = Transaction::deserialize(data)?;
                Ok(Message::Tx(tx))
            }
            "ping" => {
                if data.len() < 8 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::UnexpectedEof,
                    ));
                }
                let nonce = u64::from_le_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ]);
                Ok(Message::Ping(nonce))
            }
            "pong" => {
                if data.len() < 8 {
                    return Err(MessageError::DeserializeError(
                        DeserializeError::UnexpectedEof,
                    ));
                }
                let nonce = u64::from_le_bytes([
                    data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
                ]);
                Ok(Message::Pong(nonce))
            }
            "reject" => {
                let (message, rest) = read_var_string(data)?;
                let (reason, _) = read_var_string(rest)?;
                Ok(Message::Reject { message, reason })
            }
            cmd => Err(MessageError::UnknownCommand(cmd.to_string())),
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Compute the 4-byte checksum of data (first 4 bytes of SHA3-512).
fn compute_checksum(data: &[u8]) -> [u8; 4] {
    let hash = crate::crypto::hash(data);
    let mut checksum = [0u8; 4];
    checksum.copy_from_slice(&hash.as_bytes()[..4]);
    checksum
}

/// Write a variable-length integer.
fn write_var_int(buf: &mut Vec<u8>, value: u64) {
    if value < 0xFD {
        buf.push(value as u8);
    } else if value <= 0xFFFF {
        buf.push(0xFD);
        buf.extend_from_slice(&(value as u16).to_le_bytes());
    } else if value <= 0xFFFFFFFF {
        buf.push(0xFE);
        buf.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        buf.push(0xFF);
        buf.extend_from_slice(&value.to_le_bytes());
    }
}

/// Read a variable-length integer.
fn read_var_int(data: &[u8]) -> Result<(u64, &[u8]), DeserializeError> {
    if data.is_empty() {
        return Err(DeserializeError::UnexpectedEof);
    }
    match data[0] {
        0..=0xFC => Ok((data[0] as u64, &data[1..])),
        0xFD => {
            if data.len() < 3 {
                return Err(DeserializeError::UnexpectedEof);
            }
            let value = u16::from_le_bytes([data[1], data[2]]);
            Ok((value as u64, &data[3..]))
        }
        0xFE => {
            if data.len() < 5 {
                return Err(DeserializeError::UnexpectedEof);
            }
            let value = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
            Ok((value as u64, &data[5..]))
        }
        0xFF => {
            if data.len() < 9 {
                return Err(DeserializeError::UnexpectedEof);
            }
            let value = u64::from_le_bytes([
                data[1], data[2], data[3], data[4], data[5], data[6], data[7], data[8],
            ]);
            Ok((value, &data[9..]))
        }
    }
}

/// Write a variable-length string.
fn write_var_string(buf: &mut Vec<u8>, s: &str) {
    write_var_int(buf, s.len() as u64);
    buf.extend_from_slice(s.as_bytes());
}

/// Read a variable-length string.
fn read_var_string(data: &[u8]) -> Result<(String, &[u8]), DeserializeError> {
    let (len, rest) = read_var_int(data)?;
    // Explicit size limit for defense in depth
    if len > MAX_STRING_SIZE as u64 {
        return Err(DeserializeError::LengthOverflow);
    }
    if rest.len() < len as usize {
        return Err(DeserializeError::UnexpectedEof);
    }
    let s = std::str::from_utf8(&rest[..len as usize])
        .map_err(|_| DeserializeError::InvalidData("invalid utf8 string".to_string()))?;
    Ok((s.to_string(), &rest[len as usize..]))
}

/// Serialize a socket address.
fn serialize_socket_addr(buf: &mut Vec<u8>, addr: &SocketAddr) {
    match addr {
        SocketAddr::V4(v4) => {
            buf.push(4); // IPv4 marker
            buf.extend_from_slice(&v4.ip().octets());
            buf.extend_from_slice(&v4.port().to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            buf.push(6); // IPv6 marker
            buf.extend_from_slice(&v6.ip().octets());
            buf.extend_from_slice(&v6.port().to_be_bytes());
        }
    }
}

/// Deserialize a socket address.
fn deserialize_socket_addr(data: &[u8]) -> Result<(SocketAddr, &[u8]), DeserializeError> {
    if data.is_empty() {
        return Err(DeserializeError::UnexpectedEof);
    }
    match data[0] {
        4 => {
            if data.len() < 7 {
                return Err(DeserializeError::UnexpectedEof);
            }
            let ip = std::net::Ipv4Addr::new(data[1], data[2], data[3], data[4]);
            let port = u16::from_be_bytes([data[5], data[6]]);
            Ok((SocketAddr::from((ip, port)), &data[7..]))
        }
        6 => {
            if data.len() < 19 {
                return Err(DeserializeError::UnexpectedEof);
            }
            let mut octets = [0u8; 16];
            octets.copy_from_slice(&data[1..17]);
            let ip = std::net::Ipv6Addr::from(octets);
            let port = u16::from_be_bytes([data[17], data[18]]);
            Ok((SocketAddr::from((ip, port)), &data[19..]))
        }
        n => Err(DeserializeError::InvalidData(format!(
            "unknown address type: {n}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inv_item_roundtrip() {
        let hash = crate::crypto::hash(b"test");
        let item = InvItem::block(hash);
        let mut buf = Vec::new();
        item.serialize(&mut buf);
        let (decoded, _) = InvItem::deserialize(&buf).unwrap();
        assert_eq!(item, decoded);
    }

    #[test]
    fn test_version_message_roundtrip() {
        let msg = Message::Version {
            version: PROTOCOL_VERSION,
            services: Services::NODE_NETWORK,
            timestamp: 1234567890,
            height: 12345,
            nonce: 0xDEADBEEF,
            addr: "127.0.0.1:8333".parse().unwrap(),
            user_agent: "/pqcoin:0.1.0/".to_string(),
            relay: true,
        };
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::Version {
            version,
            services,
            height,
            nonce,
            addr,
            user_agent,
            relay,
            ..
        } = decoded
        {
            assert_eq!(version, PROTOCOL_VERSION);
            assert_eq!(services, Services::NODE_NETWORK);
            assert_eq!(height, 12345);
            assert_eq!(nonce, 0xDEADBEEF);
            assert_eq!(addr, "127.0.0.1:8333".parse::<SocketAddr>().unwrap());
            assert_eq!(user_agent, "/pqcoin:0.1.0/");
            assert!(relay);
        } else {
            panic!("expected Version message");
        }
    }

    #[test]
    fn test_ping_pong_roundtrip() {
        let nonce = 0xDEADBEEF12345678u64;
        let ping = Message::Ping(nonce);
        let serialized = ping.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::Ping(n) = decoded {
            assert_eq!(n, nonce);
        } else {
            panic!("expected Ping message");
        }
    }

    #[test]
    fn test_inv_message_roundtrip() {
        let items = vec![
            InvItem::block(crate::crypto::hash(b"block1")),
            InvItem::tx(crate::crypto::hash(b"tx1")),
        ];
        let msg = Message::Inv {
            items: items.clone(),
        };
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::Inv {
            items: decoded_items,
        } = decoded
        {
            assert_eq!(items, decoded_items);
        } else {
            panic!("expected Inv message");
        }
    }

    #[test]
    fn test_invalid_magic_rejected() {
        let mut data = Message::Verack.serialize_with_header();
        data[0] = 0xFF; // Corrupt magic
        assert!(matches!(
            Message::deserialize_with_header(&data),
            Err(MessageError::InvalidMagic)
        ));
    }

    #[test]
    fn test_checksum_mismatch_rejected() {
        let mut data = Message::Verack.serialize_with_header();
        if data.len() > 23 {
            data[23] ^= 0xFF; // Corrupt checksum
        }
        assert!(matches!(
            Message::deserialize_with_header(&data),
            Err(MessageError::ChecksumMismatch)
        ));
    }

    #[test]
    fn test_ipv6_address_roundtrip() {
        let msg = Message::Version {
            version: PROTOCOL_VERSION,
            services: Services::NODE_NETWORK,
            timestamp: 1234567890,
            height: 100,
            nonce: 0x12345678,
            addr: "[::1]:8333".parse().unwrap(),
            user_agent: "/test/".to_string(),
            relay: false,
        };
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::Version { addr, relay, .. } = decoded {
            assert_eq!(addr, "[::1]:8333".parse::<SocketAddr>().unwrap());
            assert!(!relay);
        } else {
            panic!("expected Version message");
        }
    }

    #[test]
    fn test_verack_roundtrip() {
        let msg = Message::Verack;
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        assert!(matches!(decoded, Message::Verack));
    }

    #[test]
    fn test_getaddr_roundtrip() {
        let msg = Message::GetAddr;
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        assert!(matches!(decoded, Message::GetAddr));
    }

    #[test]
    fn test_addr_roundtrip() {
        let addrs = vec![
            "127.0.0.1:8333".parse().unwrap(),
            "192.168.1.1:8334".parse().unwrap(),
            "[::1]:8335".parse().unwrap(),
        ];
        let msg = Message::Addr {
            addrs: addrs.clone(),
        };
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::Addr {
            addrs: decoded_addrs,
        } = decoded
        {
            assert_eq!(addrs, decoded_addrs);
        } else {
            panic!("expected Addr message");
        }
    }

    #[test]
    fn test_getheaders_roundtrip() {
        let locator = vec![
            crate::crypto::hash(b"block1"),
            crate::crypto::hash(b"block2"),
        ];
        let stop = crate::crypto::hash(b"stop");
        let msg = Message::GetHeaders {
            locator: locator.clone(),
            stop,
        };
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::GetHeaders {
            locator: decoded_locator,
            stop: decoded_stop,
        } = decoded
        {
            assert_eq!(locator, decoded_locator);
            assert_eq!(stop, decoded_stop);
        } else {
            panic!("expected GetHeaders message");
        }
    }

    #[test]
    fn test_reject_roundtrip() {
        let msg = Message::Reject {
            message: "tx".to_string(),
            reason: "invalid signature".to_string(),
        };
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::Reject { message, reason } = decoded {
            assert_eq!(message, "tx");
            assert_eq!(reason, "invalid signature");
        } else {
            panic!("expected Reject message");
        }
    }

    #[test]
    fn test_getdata_roundtrip() {
        let items = vec![
            InvItem::block(crate::crypto::hash(b"block1")),
            InvItem::tx(crate::crypto::hash(b"tx1")),
        ];
        let msg = Message::GetData {
            items: items.clone(),
        };
        let serialized = msg.serialize_with_header();
        let decoded = Message::deserialize_with_header(&serialized).unwrap();
        if let Message::GetData {
            items: decoded_items,
        } = decoded
        {
            assert_eq!(items, decoded_items);
        } else {
            panic!("expected GetData message");
        }
    }

    #[test]
    fn test_unknown_command_rejected() {
        // Manually craft a message with an unknown command
        let mut data = vec![];
        data.extend_from_slice(&NETWORK_MAGIC);
        data.extend_from_slice(b"unknowncmd\0\0"); // 12 bytes
        data.extend_from_slice(&0u32.to_le_bytes()); // length = 0
        let checksum = compute_checksum(&[]);
        data.extend_from_slice(&checksum);

        assert!(matches!(
            Message::deserialize_with_header(&data),
            Err(MessageError::UnknownCommand(_))
        ));
    }
}
