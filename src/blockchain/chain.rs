//! Blockchain state management.

use crate::constants::{MAX_BLOCK_SIZE, MAX_FUTURE_BLOCK_TIME, MAX_TX_INPUTS, MAX_TX_OUTPUTS};
use crate::crypto::Hash;
use crate::storage::{LmdbStorage, StorageError, StorageWrite, StorageWriteTxn};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use super::address::Address;
use super::block::Block;
use super::checkpoint::validate_checkpoint;
use super::difficulty::{calculate_new_difficulty, expected_difficulty_at, median_time_past};
pub(crate) use super::dust::dust_limit;
use super::dust::is_dust;
use super::error::BlockchainError;
use super::locking::LockingCondition;
use super::outpoint::OutPoint;
use super::reorg::{
    apply_block, apply_block_to_storage, find_block_containing_tx, reorganize_to,
    reorganize_to_with_storage,
};
use super::serialize::Serialize;
use super::utxo::Utxo;
use super::verification::verify_transactions_parallel;

/// The blockchain state, tracking all blocks and the UTXO set.
///
/// # Chain Selection
///
/// When multiple valid chains exist (forks), the chain with the most
/// cumulative proof-of-work is selected as the canonical chain. This is
/// typically the longest chain, but difficulty adjustments mean that
/// raw block count isn't sufficient.
///
/// # UTXO Set
///
/// The UTXO (Unspent Transaction Output) set tracks all outputs that
/// can still be spent. This provides efficient validation of new
/// transactions without scanning the entire blockchain.
///
/// # Storage
///
/// When storage is enabled, all mutations are persisted to LMDB via a
/// write-through pattern. In-memory HashMaps serve as caches for fast reads.
pub struct Blockchain {
    /// All known blocks, indexed by hash.
    blocks: HashMap<Hash, Block>,
    /// Block heights, indexed by hash.
    heights: HashMap<Hash, u64>,
    /// The hash of the current chain tip (best block).
    tip: Hash,
    /// The height of the current chain tip.
    tip_height: u64,
    /// The unspent transaction output set.
    utxos: HashMap<OutPoint, Utxo>,
    /// Transaction ID to block hash index for fast lookups during reorgs.
    tx_index: HashMap<Hash, Hash>,
    /// The genesis block hash.
    genesis_hash: Hash,
    /// Difficulty adjustment interval (blocks).
    difficulty_adjustment_interval: u64,
    /// Target block time (seconds).
    target_block_time: u64,
    /// Initial block reward.
    initial_reward: u64,
    /// Reward halving interval (blocks).
    halving_interval: u64,
    /// Optional persistent storage (LMDB).
    storage: Option<Arc<LmdbStorage>>,
}

impl Clone for Blockchain {
    fn clone(&self) -> Self {
        Blockchain {
            blocks: self.blocks.clone(),
            heights: self.heights.clone(),
            tip: self.tip,
            tip_height: self.tip_height,
            utxos: self.utxos.clone(),
            tx_index: self.tx_index.clone(),
            genesis_hash: self.genesis_hash,
            difficulty_adjustment_interval: self.difficulty_adjustment_interval,
            target_block_time: self.target_block_time,
            initial_reward: self.initial_reward,
            halving_interval: self.halving_interval,
            storage: self.storage.clone(),
        }
    }
}

impl Blockchain {
    /// Create a new blockchain with the given genesis block (in-memory only).
    ///
    /// # Parameters
    ///
    /// * `genesis` - The genesis block (block 0)
    /// * `difficulty_adjustment_interval` - How often to adjust difficulty (e.g., 2016 for Bitcoin)
    /// * `target_block_time` - Target seconds between blocks (e.g., 600 for Bitcoin)
    /// * `initial_reward` - Initial block reward in quanta
    /// * `halving_interval` - How often to halve the reward (e.g., 210000 for Bitcoin)
    pub fn new(
        genesis: Block,
        difficulty_adjustment_interval: u64,
        target_block_time: u64,
        initial_reward: u64,
        halving_interval: u64,
    ) -> Self {
        let genesis_hash = genesis.hash();

        let mut blocks = HashMap::new();
        let mut heights = HashMap::new();
        let mut utxos = HashMap::new();
        let mut tx_index = HashMap::new();

        // Add genesis block UTXOs and index transactions
        for (i, tx) in genesis.transactions.iter().enumerate() {
            let txid = tx.txid();
            tx_index.insert(txid, genesis_hash);
            for (j, output) in tx.outputs.iter().enumerate() {
                let outpoint = OutPoint::new(txid, j as u32);
                utxos.insert(
                    outpoint,
                    Utxo {
                        output: output.clone(),
                        height: 0,
                        is_coinbase: i == 0,
                    },
                );
            }
        }

        blocks.insert(genesis_hash, genesis);
        heights.insert(genesis_hash, 0);

        Blockchain {
            blocks,
            heights,
            tip: genesis_hash,
            tip_height: 0,
            utxos,
            tx_index,
            genesis_hash,
            difficulty_adjustment_interval,
            target_block_time,
            initial_reward,
            halving_interval,
            storage: None,
        }
    }

    /// Open or create a blockchain with persistent storage.
    ///
    /// If storage exists and is initialized, loads state from storage.
    /// If storage is empty, initializes it with the provided genesis block.
    ///
    /// # Parameters
    ///
    /// * `storage_path` - Path to the LMDB storage directory
    /// * `genesis` - The genesis block (used if storage is empty)
    /// * `difficulty_adjustment_interval` - How often to adjust difficulty
    /// * `target_block_time` - Target seconds between blocks
    /// * `initial_reward` - Initial block reward in quanta
    /// * `halving_interval` - How often to halve the reward
    pub fn open<P: AsRef<Path>>(
        storage_path: P,
        genesis: Block,
        difficulty_adjustment_interval: u64,
        target_block_time: u64,
        initial_reward: u64,
        halving_interval: u64,
    ) -> Result<Self, BlockchainError> {
        let storage = LmdbStorage::open(storage_path)?;

        if storage.is_initialized()? {
            // Load existing state from storage
            Self::load_from_storage(storage)
        } else {
            // Initialize new storage with genesis
            storage.init_genesis(
                &genesis,
                difficulty_adjustment_interval,
                target_block_time,
                initial_reward,
                halving_interval,
            )?;
            Self::load_from_storage(storage)
        }
    }

    /// Load blockchain state from initialized storage.
    fn load_from_storage(storage: LmdbStorage) -> Result<Self, BlockchainError> {
        let config = storage
            .load_config()?
            .ok_or_else(|| StorageError::Corruption("storage not initialized".to_string()))?;

        // Load all data into in-memory caches
        let blocks: HashMap<Hash, Block> = storage.load_all_blocks()?.into_iter().collect();

        let heights: HashMap<Hash, u64> = storage.load_all_heights()?.into_iter().collect();

        let utxos: HashMap<OutPoint, Utxo> = storage.load_all_utxos()?.into_iter().collect();

        let tx_index: HashMap<Hash, Hash> = storage.load_all_tx_index()?.into_iter().collect();

        tracing::info!(
            blocks = blocks.len(),
            utxos = utxos.len(),
            tip_height = config.tip_height,
            "loaded blockchain from storage"
        );

        Ok(Blockchain {
            blocks,
            heights,
            tip: config.tip_hash,
            tip_height: config.tip_height,
            utxos,
            tx_index,
            genesis_hash: config.genesis_hash,
            difficulty_adjustment_interval: config.difficulty_interval,
            target_block_time: config.target_block_time,
            initial_reward: config.initial_reward,
            halving_interval: config.halving_interval,
            storage: Some(Arc::new(storage)),
        })
    }

    /// Check if this blockchain has persistent storage enabled.
    pub fn has_storage(&self) -> bool {
        self.storage.is_some()
    }

    /// Get the genesis block.
    pub fn genesis(&self) -> &Block {
        self.blocks.get(&self.genesis_hash).unwrap()
    }

    /// Get the current chain tip (best block).
    pub fn tip(&self) -> &Block {
        self.blocks.get(&self.tip).unwrap()
    }

    /// Get the current chain tip hash.
    pub fn tip_hash(&self) -> Hash {
        self.tip
    }

    /// Get the current chain height.
    pub fn height(&self) -> u64 {
        self.tip_height
    }

    /// Get a block by its hash.
    pub fn get_block(&self, hash: &Hash) -> Option<&Block> {
        self.blocks.get(hash)
    }

    /// Get a block's height.
    pub fn get_height(&self, hash: &Hash) -> Option<u64> {
        self.heights.get(hash).copied()
    }

    /// Alias for get_height (used by sync module).
    pub fn height_of(&self, hash: &Hash) -> Option<u64> {
        self.get_height(hash)
    }

    /// Check if we have a block.
    pub fn has_block(&self, hash: &Hash) -> bool {
        self.blocks.contains_key(hash)
    }

    /// Get the block hash at a specific height.
    ///
    /// This walks back from the tip, so it's O(height) in the worst case.
    pub fn hash_at_height(&self, target_height: u64) -> Option<Hash> {
        if target_height > self.tip_height {
            return None;
        }

        let mut current_hash = self.tip;
        let mut current_height = self.tip_height;

        while current_height > target_height {
            if let Some(block) = self.blocks.get(&current_hash) {
                current_hash = block.header.prev_hash;
                current_height -= 1;
            } else {
                return None;
            }
        }

        Some(current_hash)
    }

    /// Alias for hash_at_height (used by sync module).
    pub fn block_hash_at_height(&self, height: u64) -> Option<Hash> {
        self.hash_at_height(height)
    }

    /// Get a UTXO by its outpoint.
    pub fn get_utxo(&self, outpoint: &OutPoint) -> Option<&Utxo> {
        self.utxos.get(outpoint)
    }

    /// Calculate the block reward for a given height.
    pub fn block_reward(&self, height: u64) -> u64 {
        let halvings = height / self.halving_interval;
        if halvings >= 64 {
            0
        } else {
            self.initial_reward >> halvings
        }
    }

    /// Calculate the expected difficulty for a new block.
    ///
    /// Difficulty is adjusted every `difficulty_adjustment_interval` blocks
    /// to maintain the target block time.
    pub fn next_difficulty(&self) -> u32 {
        let tip = self.tip();

        // If we're not at an adjustment boundary, keep the same difficulty
        if (self.tip_height + 1) % self.difficulty_adjustment_interval != 0 {
            return tip.header.difficulty_bits;
        }

        // Find the block at the start of this adjustment period
        let period_start_height = self
            .tip_height
            .saturating_sub(self.difficulty_adjustment_interval - 1);
        let mut block_hash = self.tip;

        // Walk back to find the period start block
        for _ in 0..(self.tip_height - period_start_height) {
            if let Some(block) = self.blocks.get(&block_hash) {
                block_hash = block.header.prev_hash;
            } else {
                return tip.header.difficulty_bits;
            }
        }

        let period_start = match self.blocks.get(&block_hash) {
            Some(b) => b,
            None => return tip.header.difficulty_bits,
        };

        calculate_new_difficulty(
            tip.header.timestamp,
            period_start.header.timestamp,
            tip.header.difficulty_bits,
            self.difficulty_adjustment_interval,
            self.target_block_time,
        )
    }

    /// Add a block to the chain.
    ///
    /// Returns `Ok(true)` if the block extended the main chain,
    /// `Ok(false)` if it was added to a side chain, or an error if invalid.
    ///
    /// When storage is enabled, all mutations are persisted atomically.
    /// The operation sequence ensures crash-safety:
    /// 1. Add block to in-memory block/height caches (metadata only, reconstructible)
    /// 2. Persist all state changes to storage in a single transaction
    /// 3. Update in-memory UTXO/tx_index caches (critical state)
    ///
    /// If a crash occurs after step 1 but before step 2, on restart we load from
    /// storage which doesn't have the block - state is consistent.
    /// If a crash occurs after step 2 but before step 3, on restart we load the
    /// complete state from storage - state is consistent.
    pub fn add_block(&mut self, block: Block) -> Result<bool, BlockchainError> {
        let block_hash = block.hash();

        // Already have this block?
        if self.blocks.contains_key(&block_hash) {
            return Ok(false);
        }

        // Check block size limit to prevent memory exhaustion attacks
        let block_size = block.to_bytes().len();
        if block_size > MAX_BLOCK_SIZE {
            return Err(BlockchainError::BlockTooLarge);
        }

        // Check previous block exists
        let prev_height = self
            .heights
            .get(&block.header.prev_hash)
            .ok_or(BlockchainError::UnknownPreviousBlock)?;
        let height = prev_height + 1;

        // Validate checkpoint if one exists at this height
        validate_checkpoint(height, block_hash)?;

        // Verify timestamp using Median-Time-Past (MTP) rule (BIP 113)
        let mtp = median_time_past(block.header.prev_hash, self.genesis_hash, &self.blocks);
        if block.header.timestamp <= mtp {
            return Err(BlockchainError::InvalidTimestamp);
        }

        // Verify timestamp: must not be more than MAX_FUTURE_BLOCK_TIME in the future
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if block.header.timestamp > current_time + MAX_FUTURE_BLOCK_TIME {
            return Err(BlockchainError::InvalidTimestamp);
        }

        // Verify difficulty matches expected value for this height
        if height > 0 {
            let expected_difficulty = expected_difficulty_at(
                height,
                block.header.prev_hash,
                &self.blocks,
                self.difficulty_adjustment_interval,
                self.target_block_time,
            );
            if block.header.difficulty_bits != expected_difficulty {
                return Err(BlockchainError::InvalidDifficulty);
            }
        }

        // Verify proof of work
        if !block.header.check_pow() {
            return Err(BlockchainError::InvalidProofOfWork);
        }

        // Verify merkle root
        if !block.verify_merkle_root() {
            return Err(BlockchainError::InvalidMerkleRoot);
        }

        // Verify transactions
        self.validate_block_transactions(&block, height)?;

        // Block is valid; determine if it extends the chain or triggers a reorg
        let extends_main_chain = height > self.tip_height;
        let simple_extension = extends_main_chain && block.header.prev_hash == self.tip;

        // STEP 1: Add block to in-memory block/height caches FIRST.
        // This is necessary because reorg functions need to walk the chain including
        // this block. These are metadata caches that can be reconstructed from storage
        // on restart, so updating them before storage commit is safe.
        self.blocks.insert(block_hash, block.clone());
        self.heights.insert(block_hash, height);

        // STEP 2: Persist to storage (if enabled) in a single atomic transaction
        if let Some(storage) = &self.storage {
            let storage_result = (|| -> Result<(), BlockchainError> {
                let mut wtxn = storage.write_txn()?;

                // Store the block and its height
                wtxn.put_block(&block_hash, &block)?;
                wtxn.put_height(&block_hash, height)?;

                if extends_main_chain {
                    if simple_extension {
                        // Simple case: extends current chain
                        apply_block_to_storage(&mut wtxn, &block, height, &block_hash)?;
                    } else {
                        // Reorg needed: use the now-updated blocks/heights maps
                        reorganize_to_with_storage(
                            &mut wtxn,
                            self.tip,
                            block_hash,
                            self.genesis_hash,
                            &self.blocks,
                            &self.heights,
                        )?;
                    }
                    wtxn.set_tip(&block_hash, height)?;
                }

                // Commit storage transaction - all or nothing
                wtxn.commit()?;
                Ok(())
            })();

            // If storage failed, rollback in-memory block/height changes
            if let Err(e) = storage_result {
                self.blocks.remove(&block_hash);
                self.heights.remove(&block_hash);
                return Err(e);
            }
        }

        // STEP 3: Update in-memory UTXO and tx_index caches
        // These are the critical state caches. We only update them after storage
        // commit succeeds (or if there's no storage).
        if extends_main_chain {
            if simple_extension {
                // Simple case: extends current chain
                apply_block(&block, height, &mut self.utxos, &mut self.tx_index);
            } else {
                // Reorg needed: blocks/heights already have the new block
                reorganize_to(
                    self.tip,
                    block_hash,
                    height,
                    self.genesis_hash,
                    &self.blocks,
                    &self.heights,
                    &mut self.utxos,
                    &mut self.tx_index,
                )?;
            }
            self.tip = block_hash;
            self.tip_height = height;
            Ok(true)
        } else {
            // Side chain block, stored but doesn't change tip
            Ok(false)
        }
    }

    /// Validate all transactions in a block.
    fn validate_block_transactions(
        &self,
        block: &Block,
        height: u64,
    ) -> Result<(), BlockchainError> {
        if block.transactions.is_empty() {
            return Err(BlockchainError::EmptyBlock);
        }

        // First transaction must be coinbase
        if !block.transactions[0].is_coinbase() {
            return Err(BlockchainError::InvalidCoinbase);
        }

        // Verify all other transactions are not coinbase
        for tx in block.transactions.iter().skip(1) {
            if tx.is_coinbase() {
                return Err(BlockchainError::InvalidCoinbase);
            }
        }

        // Validate transaction input/output counts
        for tx in &block.transactions {
            if tx.inputs.len() > MAX_TX_INPUTS {
                return Err(BlockchainError::TooManyInputs(tx.inputs.len()));
            }
            if tx.outputs.len() > MAX_TX_OUTPUTS {
                return Err(BlockchainError::TooManyOutputs(tx.outputs.len()));
            }
        }

        // Check for dust outputs (skip coinbase transaction)
        for tx in block.transactions.iter().skip(1) {
            for (index, output) in tx.outputs.iter().enumerate() {
                if is_dust(output) {
                    return Err(BlockchainError::DustOutput {
                        index,
                        amount: output.amount,
                        limit: dust_limit(&output.condition),
                    });
                }
            }
        }

        // Track spent outputs to detect double-spends within the block
        let mut spent_in_block = HashMap::new();
        for tx in block.transactions.iter().skip(1) {
            for input in &tx.inputs {
                if spent_in_block.contains_key(&input.outpoint) {
                    return Err(BlockchainError::DoubleSpend(input.outpoint));
                }
                if !self.utxos.contains_key(&input.outpoint) {
                    return Err(BlockchainError::MissingInput(input.outpoint));
                }
                spent_in_block.insert(input.outpoint, ());
            }
        }

        // Verify all non-coinbase transactions in parallel
        let non_coinbase_txs: Vec<_> = block.transactions.iter().skip(1).cloned().collect();
        let input_sums = verify_transactions_parallel(&non_coinbase_txs, height, &self.utxos)?;

        // Calculate total fees
        let mut total_fees = 0u64;
        for (tx, input_sum) in non_coinbase_txs.iter().zip(input_sums.iter()) {
            let output_sum = tx.total_output();
            let fee = input_sum
                .checked_sub(output_sum)
                .ok_or(BlockchainError::InsufficientInputs)?;
            total_fees = total_fees
                .checked_add(fee)
                .ok_or(BlockchainError::FeeOverflow)?;
        }

        // Verify coinbase amount
        let expected_reward = self.block_reward(height);
        let coinbase_output = block.transactions[0].total_output();
        let max_coinbase = expected_reward
            .checked_add(total_fees)
            .ok_or(BlockchainError::FeeOverflow)?;
        if coinbase_output > max_coinbase {
            return Err(BlockchainError::InvalidCoinbase);
        }

        Ok(())
    }

    /// Get the chain of block hashes from genesis to tip.
    pub fn chain(&self) -> Vec<Hash> {
        let mut chain = Vec::with_capacity(self.tip_height as usize + 1);
        let mut current = self.tip;

        while let Some(block) = self.blocks.get(&current) {
            chain.push(current);
            if current == self.genesis_hash {
                break;
            }
            current = block.header.prev_hash;
        }

        chain.reverse();
        chain
    }

    /// Get all UTXOs for a given address.
    pub fn utxos_for_address(&self, address: &Address) -> Vec<(OutPoint, &Utxo)> {
        self.utxos
            .iter()
            .filter(|(_, utxo)| match &utxo.output.condition {
                LockingCondition::P2PKH(addr) => addr == address,
                _ => false,
            })
            .map(|(op, utxo)| (*op, utxo))
            .collect()
    }

    /// Get the total balance for an address.
    pub fn balance(&self, address: &Address) -> u64 {
        self.utxos_for_address(address)
            .iter()
            .map(|(_, utxo)| utxo.output.amount)
            .sum()
    }

    /// Get an iterator over all blocks in the blockchain.
    pub fn all_blocks(&self) -> impl Iterator<Item = &Block> {
        self.blocks.values()
    }

    /// Check if a transaction exists in the blockchain.
    pub fn has_transaction(&self, txid: &Hash) -> bool {
        find_block_containing_tx(txid, &self.tx_index, &self.blocks).is_some()
    }
}

impl std::fmt::Debug for Blockchain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blockchain")
            .field("height", &self.tip_height)
            .field("tip", &format!("{}...", &self.tip.to_hex()[..16]))
            .field("blocks", &self.blocks.len())
            .field("utxos", &self.utxos.len())
            .finish()
    }
}
