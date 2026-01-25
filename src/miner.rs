//! Block mining.
//!
//! Simple CPU miner for creating new blocks.
//!
//! # Transaction Selection
//!
//! The miner selects transactions from the mempool using fee-rate prioritization:
//! transactions are sorted by `fee / serialized_size` in descending order. This
//! maximizes miner revenue by including the highest-paying transactions first.
//!
//! See [`Mempool::get_block_txs_with_fees()`] for the sorting implementation.

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
///
/// # Nonce Exhaustion Handling
///
/// The mining process uses an extraNonce mechanism to handle the case where the
/// primary u64 nonce space is exhausted (extremely unlikely but theoretically possible
/// with ASICs or long mining sessions). When the nonce wraps around:
/// 1. Increment the extraNonce counter
/// 2. Regenerate the coinbase transaction with the new extraNonce
/// 3. Recompute the merkle root
/// 4. Continue mining with the new block template
///
/// This matches Bitcoin's approach where the coinbase scriptSig contains an extraNonce.
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

    // Get transactions from mempool, sorted by fee rate (highest first)
    let mempool_txs = mempool.get_block_txs_with_fees(MAX_BLOCK_TXS, blockchain);

    // Calculate fees
    let fees: u64 = mempool_txs.iter().map(|tx| calculate_fee(tx, blockchain)).sum();

    // Get difficulty
    let difficulty_bits = blockchain.next_difficulty();

    // ExtraNonce for handling nonce exhaustion
    let mut extra_nonce: u64 = 0;

    loop {
        // Create coinbase transaction with extraNonce
        let coinbase = create_coinbase_with_extra_nonce(height, reward + fees, miner_address, extra_nonce);
        let mut txs = vec![coinbase];
        txs.extend(mempool_txs.clone());

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

        // Mine with current extraNonce
        loop {
            if stop.load(Ordering::Relaxed) {
                return MineResult::Stopped;
            }

            if header.check_pow() {
                let block = Block::new(header, txs);
                return MineResult::Success(block);
            }

            // Check for nonce exhaustion (wrapped around)
            if header.nonce == u64::MAX {
                // Increment extraNonce and regenerate block template
                extra_nonce = extra_nonce.wrapping_add(1);
                break;
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
}

/// Create a coinbase transaction with an extraNonce value.
///
/// The extraNonce is included in the coinbase data alongside the block height,
/// providing additional entropy when the primary nonce space is exhausted.
fn create_coinbase_with_extra_nonce(height: u64, reward: u64, recipient: Address, extra_nonce: u64) -> Transaction {
    use crate::blockchain::{TxInput, TxOutput, Witness};

    // Coinbase data includes height (8 bytes) and extraNonce (8 bytes)
    let mut coinbase_data = height.to_le_bytes().to_vec();
    coinbase_data.extend_from_slice(&extra_nonce.to_le_bytes());

    Transaction {
        version: Transaction::CURRENT_VERSION,
        inputs: vec![TxInput {
            outpoint: crate::blockchain::OutPoint::null(),
            witness: Witness::Coinbase(coinbase_data),
        }],
        outputs: vec![TxOutput::p2pkh(reward, recipient)],
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
