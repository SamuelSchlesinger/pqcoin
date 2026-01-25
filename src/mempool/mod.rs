//! Transaction mempool.
//!
//! Holds unconfirmed transactions waiting to be included in blocks.
//!
//! # Transaction Dependencies
//!
//! The mempool does **not** support spending outputs created by other unconfirmed
//! transactions (often called "CPFP" or "chained transactions" in Bitcoin terminology).
//!
//! All inputs must reference outputs already confirmed in the blockchain. This is
//! a deliberate simplification to reduce mempool complexity and validation overhead.
//!
//! ## Implications
//!
//! - Wallets must wait for confirmation before spending transaction outputs
//! - Fee-bumping via CPFP is not supported
//! - Batch operations must be sequential (not parallel dependency chains)
//!
//! ## Rationale
//!
//! pqcoin prioritizes simplicity and correctness over Bitcoin feature-parity.
//! Most real-world usage patterns (users sending transactions sequentially) are
//! unaffected by this limitation.

mod entry;
mod error;
mod fees;
mod validation;

#[cfg(test)]
mod tests;

// Re-export public types
pub use entry::MempoolEntry;
pub use error::MempoolError;

use crate::blockchain::{Blockchain, OutPoint, Serialize, Transaction};
use crate::constants::{MIN_RELAY_FEE, TX_EXPIRY_BLOCKS};
use crate::crypto::Hash;
use std::collections::{HashMap, HashSet};

use validation::validate_mempool_tx;

/// Maximum transactions in mempool.
pub const MAX_MEMPOOL_SIZE: usize = 5000;

/// Transaction mempool.
#[derive(Clone)]
pub struct Mempool {
    /// Transactions indexed by txid.
    pub(crate) txs: HashMap<Hash, MempoolEntry>,
    /// Track which outpoints are spent by mempool txs (to detect conflicts).
    spent_outpoints: HashMap<OutPoint, Hash>,
}

impl Mempool {
    pub fn new() -> Self {
        Self {
            txs: HashMap::new(),
            spent_outpoints: HashMap::new(),
        }
    }

    /// Number of transactions in the mempool.
    pub fn len(&self) -> usize {
        self.txs.len()
    }

    /// Check if mempool is empty.
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    /// Check if a transaction is in the mempool.
    pub fn contains(&self, txid: &Hash) -> bool {
        self.txs.contains_key(txid)
    }

    /// Get a transaction by txid.
    pub fn get(&self, txid: &Hash) -> Option<&Transaction> {
        self.txs.get(txid).map(|entry| &entry.tx)
    }

    /// Get a mempool entry by txid.
    pub fn get_entry(&self, txid: &Hash) -> Option<&MempoolEntry> {
        self.txs.get(txid)
    }

    /// Add a transaction to the mempool.
    ///
    /// Returns true if added, false if already present or conflicts.
    /// The `current_height` parameter is used to track when the transaction was added.
    pub fn add(
        &mut self,
        tx: Transaction,
        blockchain: &Blockchain,
        current_height: u64,
    ) -> Result<bool, MempoolError> {
        let txid = tx.txid();

        // Already in mempool?
        if self.txs.contains_key(&txid) {
            return Ok(false);
        }

        // Mempool full?
        if self.txs.len() >= MAX_MEMPOOL_SIZE {
            return Err(MempoolError::Full);
        }

        // Coinbase not allowed in mempool
        if tx.is_coinbase() {
            return Err(MempoolError::CoinbaseNotAllowed);
        }

        // Check minimum relay fee
        if let Some(fee) = self.calculate_fee(&tx, blockchain) {
            let tx_size = tx.to_bytes().len() as u64;
            let fee_rate = if tx_size > 0 { fee / tx_size } else { 0 };
            if fee_rate < MIN_RELAY_FEE {
                return Err(MempoolError::FeeTooLow {
                    got: fee_rate,
                    min: MIN_RELAY_FEE,
                });
            }
        }

        // Check for double-spends within mempool
        for input in &tx.inputs {
            if let Some(conflicting_txid) = self.spent_outpoints.get(&input.outpoint) {
                return Err(MempoolError::DoubleSpend(*conflicting_txid));
            }
        }

        // Validate transaction against blockchain
        validate_mempool_tx(&tx, blockchain, self)?;

        // Add to mempool
        for input in &tx.inputs {
            self.spent_outpoints.insert(input.outpoint, txid);
        }
        self.txs.insert(txid, MempoolEntry::new(tx, current_height));

        Ok(true)
    }

    /// Remove a transaction from the mempool.
    pub fn remove(&mut self, txid: &Hash) -> Option<Transaction> {
        if let Some(entry) = self.txs.remove(txid) {
            for input in &entry.tx.inputs {
                self.spent_outpoints.remove(&input.outpoint);
            }
            Some(entry.tx)
        } else {
            None
        }
    }

    /// Remove expired transactions from the mempool.
    ///
    /// Transactions that have been in the mempool for more than TX_EXPIRY_BLOCKS
    /// will be removed.
    pub fn cleanup_expired(&mut self, current_height: u64) {
        let expired_txids: Vec<Hash> = self
            .txs
            .iter()
            .filter(|(_, entry)| {
                current_height.saturating_sub(entry.added_height) >= TX_EXPIRY_BLOCKS
            })
            .map(|(txid, _)| *txid)
            .collect();

        for txid in expired_txids {
            self.remove(&txid);
        }
    }

    /// Remove transactions that are now in a block.
    pub fn remove_confirmed(&mut self, txs: &[Transaction]) {
        for tx in txs {
            self.remove(&tx.txid());
        }
    }

    /// Remove transactions that conflict with a block (double-spends).
    pub fn remove_conflicts(&mut self, txs: &[Transaction]) {
        let mut to_remove = HashSet::new();

        for tx in txs {
            for input in &tx.inputs {
                if let Some(conflicting_txid) = self.spent_outpoints.get(&input.outpoint) {
                    to_remove.insert(*conflicting_txid);
                }
            }
        }

        for txid in to_remove {
            self.remove(&txid);
        }
    }

    /// Get all transaction IDs.
    pub fn txids(&self) -> Vec<Hash> {
        self.txs.keys().copied().collect()
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}
