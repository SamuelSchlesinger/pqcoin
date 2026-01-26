//! # Blockchain Data Structures
//!
//! This module implements the core blockchain data structures for pqcoin, a proof-of-work
//! cryptocurrency using post-quantum cryptographic primitives.
//!
//! ## Design Overview
//!
//! The design follows Bitcoin's UTXO (Unspent Transaction Output) model with simplifications:
//!
//! - **No scripting language**: Instead of Bitcoin's Script, we support only two output types:
//!   - Pay-to-Public-Key-Hash (P2PKH): Single signature required
//!   - M-of-N Multisig: Multiple signatures required from a set of public keys
//!
//! - **Post-quantum signatures**: All signatures use ML-DSA-87 (FIPS 204)
//!
//! - **SHA3-512 hashing**: All hashes use SHA3-512 (FIPS 202)
//!
//! ## Data Flow
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │ Transaction │────▶│    Block    │────▶│  Blockchain │
//! │  (UTXO)     │     │  (PoW)      │     │  (Chain)    │
//! └─────────────┘     └─────────────┘     └─────────────┘
//! ```
//!
//! ## Serialization
//!
//! All structures implement deterministic binary serialization via the [`Serialize`] and
//! [`Deserialize`] traits. The format is compact and uses little-endian byte order for
//! multi-byte integers.
//!
//! ## Performance Optimizations
//!
//! ### Transaction Index
//!
//! The [`Blockchain`] struct maintains a `tx_index` mapping transaction IDs to block hashes.
//! This enables O(1) lookups when finding which block contains a transaction, which is
//! critical for efficient chain reorganizations.
//!
//! Without this index, `find_block_containing_tx()` would need to scan all blocks and all
//! transactions within each block (O(n*m) complexity). With the index, lookups are O(1).
//!
//! The index is automatically maintained:
//! - Entries are added when blocks are applied via `apply_block()`
//! - Entries are removed when blocks are unapplied via `unapply_block()`

mod address;
mod block;
mod chain;
mod checkpoint;
mod difficulty;
mod dust;
mod error;
mod genesis;
mod header;
mod input;
mod locking;
mod outpoint;
mod output;
mod reorg;
pub(crate) mod serialize;
mod transaction;
mod utxo;
mod verification;
mod witness;

#[cfg(test)]
mod tests;

// Re-export all public types
pub use address::Address;
pub use block::Block;
pub use chain::Blockchain;
pub(crate) use dust::dust_limit;
pub use error::BlockchainError;
pub use genesis::create_genesis_block;
pub use header::BlockHeader;
pub use input::TxInput;
pub use locking::LockingCondition;
pub use outpoint::OutPoint;
pub use output::TxOutput;
pub use serialize::{Deserialize, DeserializeError, Serialize, read_var_int};
pub use transaction::Transaction;
pub use utxo::Utxo;
pub use witness::Witness;
