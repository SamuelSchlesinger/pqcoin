//! Centralized constants for pqcoin.
//!
//! This module provides a single source of truth for all protocol, network,
//! and mining constants used throughout the codebase.

// ============================================================================
// Protocol Constants
// ============================================================================

/// Maximum number of transactions allowed in a single block (excluding coinbase).
pub const MAX_BLOCK_TXS: usize = 1000;

/// Maximum number of inputs allowed in a single transaction.
pub const MAX_TX_INPUTS: usize = 10_000;

/// Maximum number of outputs allowed in a single transaction.
pub const MAX_TX_OUTPUTS: usize = 10_000;

/// Maximum number of public keys allowed in a multisig output.
/// Limited to 15 to prevent massive witness sizes with post-quantum signatures,
/// which are significantly larger than classical ECDSA signatures.
pub const MAX_MULTISIG_KEYS: usize = 15;

/// Maximum size in bytes for variable-length serialized data.
pub const MAX_SERIALIZE_BYTES: usize = 1_000_000;

/// Maximum serialized block size in bytes (16 MB).
/// Increased from 8 MB to accommodate larger post-quantum signatures.
pub const MAX_BLOCK_SIZE: usize = 16 * 1024 * 1024;

/// Number of blocks before coinbase outputs can be spent.
pub const COINBASE_MATURITY: u64 = 100;

/// Bitmask for extracting the coefficient from difficulty bits.
pub const DIFFICULTY_COEFFICIENT_MASK: u32 = 0x00FFFFFF;

/// Maximum allowed time (in seconds) that a block timestamp can be in the future.
pub const MAX_FUTURE_BLOCK_TIME: u64 = 2 * 60 * 60; // 2 hours

/// Maximum reorg depth allowed. Prevents long-range attacks.
/// Set to 100 blocks to match Bitcoin-like behavior while preventing deep reorgs.
pub const MAX_REORG_DEPTH: u64 = 100;

// ============================================================================
// Network Constants
// ============================================================================

/// Default port for pqcoin P2P network.
pub const DEFAULT_PORT: u16 = 8333;

/// Maximum number of peer connections.
pub const MAX_PEERS: usize = 125;

/// Maximum number of outbound connections.
pub const MAX_OUTBOUND: usize = 8;

/// Interval for ping/pong keepalive (seconds).
pub const PING_INTERVAL_SECS: u64 = 60;

/// Timeout for ping responses (seconds).
pub const PING_TIMEOUT_SECS: u64 = 30;

/// Maximum number of orphan blocks to keep.
pub const MAX_ORPHAN_BLOCKS: usize = 100;

/// Maximum number of blocks to request at once during sync.
pub const MAX_BLOCKS_IN_FLIGHT: usize = 16;

/// Maximum number of headers to keep in the pending queue.
pub const MAX_PENDING_HEADERS: usize = 10000;

/// Maximum number of headers in a single message.
pub const MAX_HEADERS_COUNT: usize = 2000;

/// Maximum connections per IP address.
pub const MAX_CONNECTIONS_PER_IP: usize = 3;

/// Maximum connections allowed per /16 subnet.
/// Limits Sybil attack effectiveness by preventing address space concentration.
pub const MAX_PER_SUBNET: usize = 2;

/// Minimum time between connection attempts from the same IP (seconds).
pub const CONNECTION_RATE_LIMIT_SECS: u64 = 1;

/// Ban duration for misbehaving peers (seconds).
pub const BAN_DURATION_SECS: u64 = 24 * 60 * 60; // 24 hours

/// Ban score threshold - peer is banned when this score is reached.
pub const BAN_SCORE_THRESHOLD: u32 = 100;

/// Address relay rate limit (addresses per second).
/// Prevents address flooding attacks that could pollute peer address tables.
pub const ADDR_RELAY_RATE: f64 = 0.1;

/// Maximum burst size for address relay.
/// Allows initial burst of address messages while maintaining long-term rate limit.
pub const ADDR_RELAY_BURST: usize = 1000;

// ============================================================================
// Mining Constants
// ============================================================================

/// Default difficulty for genesis block.
/// Format: (exponent << 24) | coefficient, where target = coefficient * 2^(8*(exponent-3))
/// 0x3e00ffff: exponent=62, coefficient=0x00ffff -> requires ~40 leading zero bits
pub const DEFAULT_DIFFICULTY: u32 = 0x3e00ffff;

/// Initial block reward (50 coins in base units).
pub const INITIAL_REWARD: u64 = 50_000_000;

/// Difficulty adjustment interval (blocks).
pub const DIFFICULTY_INTERVAL: u64 = 2016;

/// Target block time in seconds (10 minutes like Bitcoin).
pub const TARGET_BLOCK_TIME: u64 = 600;

/// Halving interval (blocks).
pub const HALVING_INTERVAL: u64 = 210_000;

/// Minimum difficulty bits (maximum target).
/// This prevents difficulty from dropping below a safe floor, even during
/// periods of very low hashrate. Format: (exponent << 24) | coefficient.
/// 0x41ffffff = exponent 65 (max for 512-bit), coefficient 0xffffff
/// This effectively allows any valid PoW but protects against edge cases.
pub const MIN_DIFFICULTY_BITS: u32 = 0x41ffffff;

// ============================================================================
// Mempool Constants
// ============================================================================

/// Minimum transaction fee in quanta required for relay.
/// Prevents spam transactions and ensures miners have economic incentive to include transactions.
pub const MIN_RELAY_FEE: u64 = 1000;

/// Fee rate used for dust calculation (quanta per 1000 bytes).
/// An output is dust if spending it would cost more than its value at this rate.
/// Set to 1/10th of MIN_RELAY_FEE to allow small payments while preventing
/// truly uneconomical outputs. Dynamic dust = (spend_size * DUST_FEE_RATE) / 1000.
pub const DUST_FEE_RATE: u64 = MIN_RELAY_FEE / 10;  // 100 quanta per KB

/// Number of blocks after which an unconfirmed transaction expires (~72 hours at 10 min blocks).
/// Prevents indefinite transaction hanging and allows fee bumping after expiry.
pub const TX_EXPIRY_BLOCKS: u64 = 432;

// ============================================================================
// Testing Constants (for faster iteration)
// ============================================================================

/// Difficulty adjustment interval for testing.
pub const TEST_DIFFICULTY_INTERVAL: u64 = 10;

/// Target block time for testing (5 seconds).
pub const TEST_TARGET_BLOCK_TIME: u64 = 5;
