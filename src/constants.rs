//! Centralized constants for pqcoin.
//!
//! This module provides a single source of truth for all protocol, network,
//! and mining constants used throughout the codebase.

// ============================================================================
// Protocol Constants
// ============================================================================

/// Maximum number of transactions allowed in a single block (excluding coinbase).
/// Set to 4200 to match Bitcoin's ~7 TPS throughput (4200 txs / 600 sec = 7 TPS).
pub const MAX_BLOCK_TXS: usize = 4200;

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

/// Maximum serialized block size in bytes (32 MB).
/// Sized to accommodate ~4200 transactions at ~7.5 KB each, matching Bitcoin's ~7 TPS.
pub const MAX_BLOCK_SIZE: usize = 32 * 1024 * 1024;

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
// Peer Discovery Constants
// ============================================================================

/// Interval for peer rotation - disconnect random peer and try new one (5 minutes).
pub const PEER_ROTATION_INTERVAL_SECS: u64 = 5 * 60;

/// Interval for requesting addresses from a random peer (10 minutes).
pub const ADDR_FETCH_INTERVAL_SECS: u64 = 10 * 60;

/// Interval for attempting new outbound connections when below target (30 seconds).
pub const CONNECTION_RETRY_INTERVAL_SECS: u64 = 30;

/// Cooldown after a failed connection attempt (5 minutes).
pub const FAILED_ADDR_COOLDOWN_SECS: u64 = 5 * 60;

/// Maximum age for an address before considered stale (30 days).
pub const ADDR_MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60;

/// Delay before sending GetAddr after handshake (2 seconds).
pub const GETADDR_DELAY_SECS: u64 = 2;

/// Maximum addresses to store in address manager.
pub const MAX_KNOWN_ADDRS: usize = 10_000;

// ============================================================================
// Mining Constants
// ============================================================================

/// Default difficulty for genesis block.
/// Format: (exponent << 24) | coefficient, where target = coefficient * 2^(8*(exponent-3))
/// 0x3e00ffff: exponent=62, coefficient=0x00ffff -> requires ~40 leading zero bits
pub const DEFAULT_DIFFICULTY: u32 = 0x3e00ffff;

/// Initial block reward (50 coins in base units).
pub const INITIAL_REWARD: u64 = 50_000_000;

/// Legacy difficulty adjustment interval (blocks).
/// Note: Use `difficulty_interval_at_height()` for the graduated schedule.
pub const DIFFICULTY_INTERVAL: u64 = 2016;

/// Target block time in seconds (10 minutes like Bitcoin).
pub const TARGET_BLOCK_TIME: u64 = 600;

// ============================================================================
// Graduated Difficulty Adjustment Schedule
// ============================================================================
//
// The difficulty adjustment interval increases over time:
// - Early chain: frequent adjustments for fast stabilization
// - Mature chain: infrequent adjustments for stability (like Bitcoin)
//
// At 10-minute blocks:
// - Phase 1 (0-1000):      10 blocks  (~1.7 hours between adjustments)
// - Phase 2 (1000-10000):  50 blocks  (~8.3 hours)
// - Phase 3 (10000-100000): 200 blocks (~1.4 days)
// - Phase 4 (100000-262080): 504 blocks (~3.5 days)
// - Phase 5 (262080+):     2016 blocks (~2 weeks, Bitcoin-level)

/// Phase boundaries for graduated difficulty adjustment.
const DIFFICULTY_PHASE_1_END: u64 = 1_000;
const DIFFICULTY_PHASE_2_END: u64 = 10_000;
const DIFFICULTY_PHASE_3_END: u64 = 100_000;
const DIFFICULTY_PHASE_4_END: u64 = 262_080; // ~5 years, divisible by 2016

/// Intervals for each phase.
const DIFFICULTY_INTERVAL_PHASE_1: u64 = 10;
const DIFFICULTY_INTERVAL_PHASE_2: u64 = 50;
const DIFFICULTY_INTERVAL_PHASE_3: u64 = 200;
const DIFFICULTY_INTERVAL_PHASE_4: u64 = 504;
const DIFFICULTY_INTERVAL_PHASE_5: u64 = 2016;

/// Returns the difficulty adjustment interval for a given block height.
///
/// The interval increases over time to allow fast stabilization early on
/// while achieving Bitcoin-level stability at maturity (~5 years).
pub fn difficulty_interval_at_height(height: u64) -> u64 {
    if height < DIFFICULTY_PHASE_1_END {
        DIFFICULTY_INTERVAL_PHASE_1
    } else if height < DIFFICULTY_PHASE_2_END {
        DIFFICULTY_INTERVAL_PHASE_2
    } else if height < DIFFICULTY_PHASE_3_END {
        DIFFICULTY_INTERVAL_PHASE_3
    } else if height < DIFFICULTY_PHASE_4_END {
        DIFFICULTY_INTERVAL_PHASE_4
    } else {
        DIFFICULTY_INTERVAL_PHASE_5
    }
}

/// Returns true if the given height is a difficulty adjustment boundary.
pub fn is_difficulty_adjustment_height(height: u64) -> bool {
    if height == 0 {
        return false;
    }
    let interval = difficulty_interval_at_height(height);
    height % interval == 0
}

/// Halving interval (blocks).
pub const HALVING_INTERVAL: u64 = 210_000;

/// Minimum difficulty bits (maximum target).
/// This prevents difficulty from dropping below a safe floor, even during
/// periods of very low hashrate. Format: (exponent << 24) | coefficient.
/// 0x40ffffff = exponent 65 (max for 512-bit), coefficient 0x7fffff
/// This effectively allows any valid PoW but protects against edge cases.
/// The coefficient is limited to 0x7fffff to avoid normalization issues.
pub const MIN_DIFFICULTY_BITS: u32 = 0x40ffffff;

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
pub const DUST_FEE_RATE: u64 = MIN_RELAY_FEE / 10; // 100 quanta per KB

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

// ============================================================================
// Testnet Constants
// ============================================================================

/// Testnet genesis timestamp (2024-01-01 00:00:00 UTC).
/// Fixed timestamp ensures all testnet nodes have identical genesis.
pub const TESTNET_GENESIS_TIMESTAMP: u64 = 1704067200;

/// Testnet difficulty - very low for fast block generation.
/// Format: (exponent << 24) | coefficient
/// 0x40ffffff = exponent 65, coefficient 0x7fffff
/// This gives an extremely easy target. The coefficient must be <= 0x7fffff
/// to avoid normalization during difficulty adjustments.
pub const TESTNET_DIFFICULTY: u32 = 0x40ffffff;
