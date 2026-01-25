//! Chain reorganization logic.

use crate::constants::MAX_REORG_DEPTH;
use crate::crypto::Hash;
use crate::storage::{StorageError, StorageWriteTxn};
use std::collections::{HashMap, HashSet};

use super::block::Block;
use super::error::BlockchainError;
use super::outpoint::OutPoint;
use super::utxo::Utxo;

/// Apply a block's effects to the UTXO set and tx index (add outputs, remove inputs).
pub(crate) fn apply_block(
    block: &Block,
    height: u64,
    utxos: &mut HashMap<OutPoint, Utxo>,
    tx_index: &mut HashMap<Hash, Hash>,
) {
    let block_hash = block.hash();

    // Remove spent outputs
    for tx in block.transactions.iter().skip(1) {
        for input in &tx.inputs {
            utxos.remove(&input.outpoint);
        }
    }

    // Add new outputs and index transactions
    for (i, tx) in block.transactions.iter().enumerate() {
        let txid = tx.txid();
        tx_index.insert(txid, block_hash);
        for (j, output) in tx.outputs.iter().enumerate() {
            let outpoint = OutPoint::new(txid, j as u32);
            utxos.insert(
                outpoint,
                Utxo {
                    output: output.clone(),
                    height,
                    is_coinbase: i == 0,
                },
            );
        }
    }
}

/// Unapply a block's effects from the UTXO set and tx index (remove outputs, restore inputs).
pub(crate) fn unapply_block(
    block: &Block,
    _height: u64,
    utxos: &mut HashMap<OutPoint, Utxo>,
    tx_index: &mut HashMap<Hash, Hash>,
    blocks: &HashMap<Hash, Block>,
    heights: &HashMap<Hash, u64>,
) {
    // Remove outputs and tx index entries for this block
    for tx in block.transactions.iter() {
        let txid = tx.txid();
        tx_index.remove(&txid);
        for j in 0..tx.outputs.len() {
            let outpoint = OutPoint::new(txid, j as u32);
            utxos.remove(&outpoint);
        }
    }

    // Restore spent inputs (we need to look them up from their source transactions)
    for tx in block.transactions.iter().skip(1) {
        for input in &tx.inputs {
            // Find the transaction that created this output using the tx index
            if let Some(source_block) =
                find_block_containing_tx(&input.outpoint.txid, tx_index, blocks)
            {
                let source_height = *heights.get(&source_block.hash()).unwrap_or(&0);
                if let Some(source_tx) = source_block
                    .transactions
                    .iter()
                    .find(|t| t.txid() == input.outpoint.txid)
                {
                    if let Some(output) = source_tx.outputs.get(input.outpoint.index as usize) {
                        let is_coinbase = source_block
                            .transactions
                            .first()
                            .map(|t| t.txid() == input.outpoint.txid)
                            .unwrap_or(false);
                        utxos.insert(
                            input.outpoint,
                            Utxo {
                                output: output.clone(),
                                height: source_height,
                                is_coinbase,
                            },
                        );
                    }
                }
            }
        }
    }
}

/// Find the block containing a specific transaction.
///
/// Uses the tx_index for O(1) lookup instead of scanning all blocks.
pub(crate) fn find_block_containing_tx<'a>(
    txid: &Hash,
    tx_index: &HashMap<Hash, Hash>,
    blocks: &'a HashMap<Hash, Block>,
) -> Option<&'a Block> {
    tx_index
        .get(txid)
        .and_then(|block_hash| blocks.get(block_hash))
}

/// Find the common ancestor of two block hashes.
pub(crate) fn find_common_ancestor(
    hash1: Hash,
    hash2: Hash,
    genesis_hash: Hash,
    blocks: &HashMap<Hash, Block>,
) -> Option<Hash> {
    let mut ancestors1 = HashSet::new();
    let mut current = hash1;

    // Collect all ancestors of hash1
    while let Some(block) = blocks.get(&current) {
        ancestors1.insert(current);
        if current == genesis_hash {
            break;
        }
        current = block.header.prev_hash;
    }

    // Walk back from hash2 until we find a common ancestor
    current = hash2;
    while let Some(block) = blocks.get(&current) {
        if ancestors1.contains(&current) {
            return Some(current);
        }
        if current == genesis_hash {
            break;
        }
        current = block.header.prev_hash;
    }

    None
}

/// Get the chain of blocks from a hash back to (but not including) an ancestor.
pub(crate) fn get_chain_segment(from: Hash, to: Hash, blocks: &HashMap<Hash, Block>) -> Vec<Hash> {
    let mut chain = Vec::new();
    let mut current = from;

    while current != to {
        chain.push(current);
        if let Some(block) = blocks.get(&current) {
            current = block.header.prev_hash;
        } else {
            break;
        }
    }

    chain.reverse();
    chain
}

/// Reorganize the chain to a new tip.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reorganize_to(
    old_tip: Hash,
    new_tip: Hash,
    _new_height: u64,
    genesis_hash: Hash,
    blocks: &HashMap<Hash, Block>,
    heights: &HashMap<Hash, u64>,
    utxos: &mut HashMap<OutPoint, Utxo>,
    tx_index: &mut HashMap<Hash, Hash>,
) -> Result<(), BlockchainError> {
    // Find common ancestor
    let fork_point = find_common_ancestor(old_tip, new_tip, genesis_hash, blocks)
        .ok_or(BlockchainError::UnknownPreviousBlock)?;

    // Get blocks to unapply (old chain from tip back to fork point)
    let old_chain = get_chain_segment(old_tip, fork_point, blocks);

    // Check reorg depth to prevent long-range attacks
    let reorg_depth = old_chain.len() as u64;
    if reorg_depth > MAX_REORG_DEPTH {
        tracing::warn!(
            reorg_depth = reorg_depth,
            max_allowed = MAX_REORG_DEPTH,
            "rejecting deep reorg"
        );
        return Err(BlockchainError::ReorgTooDeep(reorg_depth));
    }

    // Get blocks to apply (new chain from fork point to new tip)
    let new_chain = get_chain_segment(new_tip, fork_point, blocks);

    tracing::info!(
        old_chain_len = old_chain.len(),
        new_chain_len = new_chain.len(),
        "performing chain reorganization"
    );

    // Unapply old blocks (in reverse order, from tip to fork point)
    for hash in old_chain.iter().rev() {
        if let Some(block) = blocks.get(hash).cloned() {
            let height = *heights.get(hash).unwrap_or(&0);
            unapply_block(&block, height, utxos, tx_index, blocks, heights);
        }
    }

    // Apply new blocks (in order, from fork point to new tip)
    for hash in &new_chain {
        if let Some(block) = blocks.get(hash).cloned() {
            let height = *heights.get(hash).unwrap_or(&0);
            apply_block(&block, height, utxos, tx_index);
        }
    }

    Ok(())
}

// ============================================================================
// Storage-aware functions
// ============================================================================

/// Apply a block's effects to storage (add outputs, remove inputs).
pub(crate) fn apply_block_to_storage<T: StorageWriteTxn>(
    wtxn: &mut T,
    block: &Block,
    height: u64,
    block_hash: &Hash,
) -> Result<(), StorageError> {
    // Remove spent outputs
    for tx in block.transactions.iter().skip(1) {
        for input in &tx.inputs {
            wtxn.delete_utxo(&input.outpoint)?;
        }
    }

    // Add new outputs and index transactions
    for (i, tx) in block.transactions.iter().enumerate() {
        let txid = tx.txid();
        wtxn.put_tx_index(&txid, block_hash)?;
        for (j, output) in tx.outputs.iter().enumerate() {
            let outpoint = OutPoint::new(txid, j as u32);
            wtxn.put_utxo(
                &outpoint,
                &Utxo {
                    output: output.clone(),
                    height,
                    is_coinbase: i == 0,
                },
            )?;
        }
    }

    Ok(())
}

/// Unapply a block's effects from storage (remove outputs, restore inputs).
pub(crate) fn unapply_block_to_storage<T: StorageWriteTxn>(
    wtxn: &mut T,
    block: &Block,
    _height: u64,
    blocks: &HashMap<Hash, Block>,
    heights: &HashMap<Hash, u64>,
    tx_index: &HashMap<Hash, Hash>,
) -> Result<(), StorageError> {
    // Remove outputs and tx index entries for this block
    for tx in block.transactions.iter() {
        let txid = tx.txid();
        wtxn.delete_tx_index(&txid)?;
        for j in 0..tx.outputs.len() {
            let outpoint = OutPoint::new(txid, j as u32);
            wtxn.delete_utxo(&outpoint)?;
        }
    }

    // Restore spent inputs (we need to look them up from their source transactions)
    for tx in block.transactions.iter().skip(1) {
        for input in &tx.inputs {
            // Find the transaction that created this output
            if let Some(source_block) =
                find_block_containing_tx(&input.outpoint.txid, tx_index, blocks)
            {
                let source_height = *heights.get(&source_block.hash()).unwrap_or(&0);
                if let Some(source_tx) = source_block
                    .transactions
                    .iter()
                    .find(|t| t.txid() == input.outpoint.txid)
                {
                    if let Some(output) = source_tx.outputs.get(input.outpoint.index as usize) {
                        let is_coinbase = source_block
                            .transactions
                            .first()
                            .map(|t| t.txid() == input.outpoint.txid)
                            .unwrap_or(false);
                        wtxn.put_utxo(
                            &input.outpoint,
                            &Utxo {
                                output: output.clone(),
                                height: source_height,
                                is_coinbase,
                            },
                        )?;
                    }
                }
            }
        }
    }

    Ok(())
}

/// Reorganize the chain in storage to a new tip.
///
/// This performs all storage mutations within the provided write transaction,
/// which should be committed by the caller for atomicity.
pub(crate) fn reorganize_to_with_storage<T: StorageWriteTxn>(
    wtxn: &mut T,
    old_tip: Hash,
    new_tip: Hash,
    genesis_hash: Hash,
    blocks: &HashMap<Hash, Block>,
    heights: &HashMap<Hash, u64>,
) -> Result<(), BlockchainError> {
    // We need to rebuild tx_index from blocks for the unapply lookups
    // This is a bit inefficient but necessary since we don't have the tx_index in storage
    let mut tx_index: HashMap<Hash, Hash> = HashMap::new();
    for (block_hash, block) in blocks {
        for tx in &block.transactions {
            tx_index.insert(tx.txid(), *block_hash);
        }
    }

    // Find common ancestor
    let fork_point = find_common_ancestor(old_tip, new_tip, genesis_hash, blocks)
        .ok_or(BlockchainError::UnknownPreviousBlock)?;

    // Get blocks to unapply (old chain from tip back to fork point)
    let old_chain = get_chain_segment(old_tip, fork_point, blocks);

    // Check reorg depth
    let reorg_depth = old_chain.len() as u64;
    if reorg_depth > MAX_REORG_DEPTH {
        return Err(BlockchainError::ReorgTooDeep(reorg_depth));
    }

    // Get blocks to apply (new chain from fork point to new tip)
    let new_chain = get_chain_segment(new_tip, fork_point, blocks);

    // Unapply old blocks (in reverse order)
    for hash in old_chain.iter().rev() {
        if let Some(block) = blocks.get(hash).cloned() {
            let height = *heights.get(hash).unwrap_or(&0);
            unapply_block_to_storage(wtxn, &block, height, blocks, heights, &tx_index)?;
            // Update tx_index as we unapply
            for tx in &block.transactions {
                tx_index.remove(&tx.txid());
            }
        }
    }

    // Apply new blocks (in order)
    for hash in &new_chain {
        if let Some(block) = blocks.get(hash).cloned() {
            let height = *heights.get(hash).unwrap_or(&0);
            apply_block_to_storage(wtxn, &block, height, hash)?;
            // Update tx_index as we apply
            for tx in &block.transactions {
                tx_index.insert(tx.txid(), *hash);
            }
        }
    }

    Ok(())
}
