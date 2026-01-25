//! Fee calculation and block template transaction selection.

use crate::blockchain::{Blockchain, Serialize, Transaction};

use super::Mempool;

impl Mempool {
    /// Get transactions for block template (up to max_txs), sorted by fee rate.
    ///
    /// Requires blockchain reference to look up input values for fee calculation.
    pub fn get_block_txs(&self, max_txs: usize) -> Vec<Transaction> {
        // Note: This is a simple version that doesn't require blockchain reference.
        // For proper fee calculation, use get_block_txs_with_fees.
        self.txs
            .values()
            .take(max_txs)
            .map(|entry| entry.tx.clone())
            .collect()
    }

    /// Get transactions for block template sorted by fee rate (fee per byte).
    ///
    /// Transactions with higher fee rates are selected first.
    pub fn get_block_txs_with_fees(
        &self,
        max_txs: usize,
        blockchain: &Blockchain,
    ) -> Vec<Transaction> {
        let mut txs_with_fee_rate: Vec<(&Transaction, f64)> = self
            .txs
            .values()
            .filter_map(|entry| {
                let fee = self.calculate_fee(&entry.tx, blockchain)?;
                let size = entry.tx.to_bytes().len();
                if size == 0 {
                    return None;
                }
                let fee_rate = fee as f64 / size as f64;
                Some((&entry.tx, fee_rate))
            })
            .collect();

        // Sort by fee rate descending (highest fee rate first)
        txs_with_fee_rate
            .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        txs_with_fee_rate
            .into_iter()
            .take(max_txs)
            .map(|(tx, _)| tx.clone())
            .collect()
    }

    /// Calculate the fee for a transaction.
    ///
    /// Returns None if any input is missing (which shouldn't happen for valid mempool txs).
    pub(crate) fn calculate_fee(&self, tx: &Transaction, blockchain: &Blockchain) -> Option<u64> {
        let mut input_sum = 0u64;

        for input in &tx.inputs {
            let utxo = blockchain.get_utxo(&input.outpoint)?;
            input_sum = input_sum.checked_add(utxo.output.amount)?;
        }

        let output_sum: u64 = tx.outputs.iter().map(|o| o.amount).sum();

        input_sum.checked_sub(output_sum)
    }
}
