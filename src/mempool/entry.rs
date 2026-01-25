//! Mempool entry containing transaction and metadata.

use crate::blockchain::Transaction;

/// Entry in the mempool containing transaction and metadata.
#[derive(Clone, Debug)]
pub struct MempoolEntry {
    /// The transaction.
    pub tx: Transaction,
    /// Block height when the transaction was added to the mempool.
    pub added_height: u64,
}

impl MempoolEntry {
    /// Create a new mempool entry.
    pub fn new(tx: Transaction, added_height: u64) -> Self {
        Self { tx, added_height }
    }
}
