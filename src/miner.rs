//! Block mining.
//!
//! Simple CPU miner for creating new blocks.

use crate::blockchain::{Address, Block, BlockHeader, Blockchain, Transaction};
use crate::constants::MAX_BLOCK_TXS;
use crate::mempool::Mempool;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Result of a mining attempt.
pub enum MineResult {
    /// Successfully mined a block.
    Success(Block),
    /// Mining was stopped.
    Stopped,
    /// No work to do (already at tip).
    NoWork,
}

/// Mine a single block.
///
/// This is a simple CPU miner that increments the nonce until finding a valid PoW.
/// Returns when a block is found or `stop` is set to true.
pub fn mine_block(
    blockchain: &Blockchain,
    mempool: &Mempool,
    miner_address: Address,
    stop: Arc<AtomicBool>,
) -> MineResult {
    let height = blockchain.height() + 1;
    let prev_block = blockchain.tip();
    let prev_hash = prev_block.hash();
    let prev_timestamp = prev_block.header.timestamp;

    // Ensure timestamp is strictly greater than previous block
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let timestamp = std::cmp::max(current_time, prev_timestamp + 1);

    // Get block reward
    let reward = blockchain.block_reward(height);

    // Get transactions from mempool
    let mut txs = mempool.get_block_txs(MAX_BLOCK_TXS);

    // Calculate fees
    let fees: u64 = txs.iter().map(|tx| calculate_fee(tx, blockchain)).sum();

    // Create coinbase transaction
    let coinbase = Transaction::coinbase(height, reward + fees, miner_address);
    txs.insert(0, coinbase);

    // Get difficulty
    let difficulty_bits = blockchain.next_difficulty();

    // Compute merkle root
    let merkle_root = Block::compute_merkle_root(&txs);

    // Create header template
    let mut header = BlockHeader {
        version: BlockHeader::CURRENT_VERSION,
        prev_hash,
        merkle_root,
        timestamp,
        difficulty_bits,
        nonce: 0,
    };

    // Mine!
    loop {
        if stop.load(Ordering::Relaxed) {
            return MineResult::Stopped;
        }

        if header.check_pow() {
            let block = Block::new(header, txs);
            return MineResult::Success(block);
        }

        header.nonce = header.nonce.wrapping_add(1);

        // Every 100k hashes, update timestamp (ensuring it stays > prev_timestamp)
        if header.nonce % 100_000 == 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            header.timestamp = std::cmp::max(now, prev_timestamp + 1);
        }
    }
}

/// Calculate transaction fee (inputs - outputs).
fn calculate_fee(tx: &Transaction, blockchain: &Blockchain) -> u64 {
    let total_in: u64 = tx
        .inputs
        .iter()
        .filter_map(|input| blockchain.get_utxo(&input.outpoint))
        .map(|utxo| utxo.output.amount)
        .sum();

    let total_out: u64 = tx.outputs.iter().map(|o| o.amount).sum();

    total_in.saturating_sub(total_out)
}

/// Background miner that runs continuously.
pub struct BackgroundMiner {
    stop: Arc<AtomicBool>,
}

impl BackgroundMiner {
    pub fn new() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Get the stop flag to share with the mining loop.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        self.stop.clone()
    }

    /// Signal the miner to stop.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Reset the stop flag (call before starting a new mining round).
    pub fn reset(&self) {
        self.stop.store(false, Ordering::Relaxed);
    }
}

impl Default for BackgroundMiner {
    fn default() -> Self {
        Self::new()
    }
}
