//! Durable storage for blockchain state.
//!
//! This module provides persistent storage using LMDB via the `heed` crate.
//! The storage layer implements a hybrid cache + write-through pattern:
//!
//! - **Reads**: Served from in-memory HashMap caches (O(1), no disk I/O)
//! - **Writes**: Write-through to LMDB for durability
//! - **Startup**: Load entire state from LMDB into memory
//!
//! # Usage
//!
//! The recommended way to use persistent storage is through [`crate::blockchain::Blockchain::open`]:
//!
//! ```no_run
//! use pqcoin::blockchain::{Blockchain, Address, create_genesis_block};
//! use pqcoin::crypto;
//!
//! let address = Address::from_hash(crypto::hash(b"miner"));
//! let genesis = create_genesis_block(0, 0x20ffffff, 50_000_000, address);
//!
//! // Open blockchain with persistent storage
//! let blockchain = Blockchain::open(
//!     "/path/to/data",
//!     genesis,
//!     2016,        // difficulty interval
//!     600,         // target block time
//!     50_000_000,  // initial reward
//!     210_000,     // halving interval
//! ).expect("failed to open storage");
//!
//! // State automatically loads from storage on startup
//! println!("Chain height: {}", blockchain.height());
//! ```
//!
//! # Transaction Soundness
//!
//! All mutations to a block's state happen within a single LMDB write transaction,
//! ensuring atomicity. If the process crashes mid-write, LMDB will roll back to
//! the last consistent state.
//!
//! # Database Schema
//!
//! | Database | Key | Value | Purpose |
//! |----------|-----|-------|---------|
//! | `blocks` | Hash (64B) | Block (variable) | All blocks by hash |
//! | `heights` | Hash (64B) | u64 (8B) | Block hash -> height |
//! | `utxos` | OutPoint (68B) | Utxo (variable) | UTXO set |
//! | `tx_index` | Hash (64B) | Hash (64B) | txid -> block hash |
//! | `metadata` | &str | bytes | Chain tip, genesis, config |

mod codecs;
mod error;
mod lmdb;

#[cfg(test)]
mod tests;

pub use codecs::{BlockCodec, HashCodec, OutPointCodec, U64Codec, UtxoCodec};
pub use error::StorageError;
pub use lmdb::LmdbStorage;

use crate::blockchain::{Block, OutPoint, Utxo};
use crate::crypto::Hash;

/// Metadata keys stored in the metadata database.
pub mod metadata_keys {
    /// The hash of the current chain tip.
    pub const TIP_HASH: &str = "tip_hash";
    /// The height of the current chain tip.
    pub const TIP_HEIGHT: &str = "tip_height";
    /// The genesis block hash.
    pub const GENESIS_HASH: &str = "genesis_hash";
    /// Difficulty adjustment interval.
    pub const DIFFICULTY_INTERVAL: &str = "difficulty_interval";
    /// Target block time in seconds.
    pub const TARGET_BLOCK_TIME: &str = "target_block_time";
    /// Initial block reward.
    pub const INITIAL_REWARD: &str = "initial_reward";
    /// Reward halving interval.
    pub const HALVING_INTERVAL: &str = "halving_interval";
}

/// Read-only storage operations.
pub trait StorageRead {
    /// Get a block by its hash.
    fn get_block(&self, hash: &Hash) -> Result<Option<Block>, StorageError>;

    /// Get a block's height.
    fn get_height(&self, hash: &Hash) -> Result<Option<u64>, StorageError>;

    /// Get a UTXO by its outpoint.
    fn get_utxo(&self, outpoint: &OutPoint) -> Result<Option<Utxo>, StorageError>;

    /// Get the block hash containing a transaction.
    fn get_tx_block(&self, txid: &Hash) -> Result<Option<Hash>, StorageError>;

    /// Get metadata by key.
    fn get_metadata(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError>;

    /// Get the chain tip hash.
    fn get_tip_hash(&self) -> Result<Option<Hash>, StorageError>;

    /// Get the chain tip height.
    fn get_tip_height(&self) -> Result<Option<u64>, StorageError>;

    /// Get the genesis hash.
    fn get_genesis_hash(&self) -> Result<Option<Hash>, StorageError>;
}

/// A write transaction that batches multiple operations.
pub trait StorageWriteTxn {
    /// Store a block.
    fn put_block(&mut self, hash: &Hash, block: &Block) -> Result<(), StorageError>;

    /// Store a block's height.
    fn put_height(&mut self, hash: &Hash, height: u64) -> Result<(), StorageError>;

    /// Store a UTXO.
    fn put_utxo(&mut self, outpoint: &OutPoint, utxo: &Utxo) -> Result<(), StorageError>;

    /// Delete a UTXO.
    fn delete_utxo(&mut self, outpoint: &OutPoint) -> Result<bool, StorageError>;

    /// Store a transaction index entry.
    fn put_tx_index(&mut self, txid: &Hash, block_hash: &Hash) -> Result<(), StorageError>;

    /// Delete a transaction index entry.
    fn delete_tx_index(&mut self, txid: &Hash) -> Result<bool, StorageError>;

    /// Set the chain tip.
    fn set_tip(&mut self, hash: &Hash, height: u64) -> Result<(), StorageError>;

    /// Store metadata.
    fn put_metadata(&mut self, key: &str, value: &[u8]) -> Result<(), StorageError>;

    /// Commit the transaction.
    fn commit(self) -> Result<(), StorageError>;

    /// Abort the transaction (explicit rollback).
    fn abort(self);
}

/// Storage operations that require write access.
pub trait StorageWrite: StorageRead {
    /// The type of write transaction.
    type WriteTxn<'a>: StorageWriteTxn
    where
        Self: 'a;

    /// Begin a write transaction.
    fn write_txn(&self) -> Result<Self::WriteTxn<'_>, StorageError>;

    /// Initialize storage with a genesis block and chain parameters.
    fn init_genesis(
        &self,
        genesis: &Block,
        difficulty_interval: u64,
        target_block_time: u64,
        initial_reward: u64,
        halving_interval: u64,
    ) -> Result<(), StorageError>;
}

/// Chain configuration parameters stored in metadata.
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// Genesis block hash.
    pub genesis_hash: Hash,
    /// Tip block hash.
    pub tip_hash: Hash,
    /// Tip height.
    pub tip_height: u64,
    /// Difficulty adjustment interval.
    pub difficulty_interval: u64,
    /// Target block time in seconds.
    pub target_block_time: u64,
    /// Initial block reward.
    pub initial_reward: u64,
    /// Reward halving interval.
    pub halving_interval: u64,
}
