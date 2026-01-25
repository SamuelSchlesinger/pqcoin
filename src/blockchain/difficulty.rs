//! Difficulty adjustment logic.

use crate::constants::{DIFFICULTY_COEFFICIENT_MASK, MIN_DIFFICULTY_BITS};
use crate::crypto::Hash;
use std::collections::HashMap;

use super::block::Block;

/// Number of blocks used to compute Median-Time-Past (MTP).
/// Bitcoin uses 11 blocks; this provides resistance to timestamp manipulation.
pub(crate) const MTP_BLOCK_COUNT: usize = 11;

/// Calculate the Median-Time-Past (MTP) for a block.
///
/// MTP is the median timestamp of the previous MTP_BLOCK_COUNT (11) blocks.
/// This is used instead of the previous block's timestamp to prevent
/// timestamp manipulation attacks (see BIP 113 in Bitcoin).
///
/// A new block's timestamp must be strictly greater than the MTP.
pub(crate) fn median_time_past(
    block_hash: Hash,
    genesis_hash: Hash,
    blocks: &HashMap<Hash, Block>,
) -> u64 {
    let mut timestamps = Vec::with_capacity(MTP_BLOCK_COUNT);
    let mut current_hash = block_hash;

    for _ in 0..MTP_BLOCK_COUNT {
        if let Some(block) = blocks.get(&current_hash) {
            timestamps.push(block.header.timestamp);
            if current_hash == genesis_hash {
                break;
            }
            current_hash = block.header.prev_hash;
        } else {
            break;
        }
    }

    if timestamps.is_empty() {
        return 0;
    }

    timestamps.sort_unstable();
    timestamps[timestamps.len() / 2]
}

/// Calculate the expected difficulty for a block at a given height.
///
/// This validates that a block has the correct difficulty based on its position
/// in the chain and the difficulty adjustment schedule.
pub(crate) fn expected_difficulty_at(
    height: u64,
    prev_hash: Hash,
    blocks: &HashMap<Hash, Block>,
    difficulty_adjustment_interval: u64,
    target_block_time: u64,
) -> u32 {
    let prev_block = match blocks.get(&prev_hash) {
        Some(b) => b,
        None => return 0, // Unknown parent, can't calculate
    };

    // If not at an adjustment boundary, use the same difficulty as parent
    if height % difficulty_adjustment_interval != 0 {
        return prev_block.header.difficulty_bits;
    }

    // At adjustment boundary - need to calculate new difficulty
    // Find the block at the start of this adjustment period
    let period_start_height = height.saturating_sub(difficulty_adjustment_interval);
    let mut block_hash = prev_hash;

    // Walk back to find the period start block
    let steps = height.saturating_sub(1).saturating_sub(period_start_height);
    for _ in 0..steps {
        if let Some(block) = blocks.get(&block_hash) {
            block_hash = block.header.prev_hash;
        } else {
            return prev_block.header.difficulty_bits;
        }
    }

    let period_start = match blocks.get(&block_hash) {
        Some(b) => b,
        None => return prev_block.header.difficulty_bits,
    };

    calculate_new_difficulty(
        prev_block.header.timestamp,
        period_start.header.timestamp,
        prev_block.header.difficulty_bits,
        difficulty_adjustment_interval,
        target_block_time,
    )
}

/// Calculate new difficulty based on actual vs target time.
pub(crate) fn calculate_new_difficulty(
    end_timestamp: u64,
    start_timestamp: u64,
    current_bits: u32,
    difficulty_adjustment_interval: u64,
    target_block_time: u64,
) -> u32 {
    // Calculate actual time taken for this period
    let actual_time = end_timestamp.saturating_sub(start_timestamp);
    let target_time = target_block_time * difficulty_adjustment_interval;

    // Clamp adjustment to 4x in either direction
    let actual_time = actual_time.max(target_time / 4).min(target_time * 4);

    // Scale the difficulty
    let exponent = current_bits >> 24;
    let coefficient = (current_bits & DIFFICULTY_COEFFICIENT_MASK) as u64;

    let scaled = (coefficient * actual_time) / target_time;

    // Minimum coefficient threshold to maintain precision
    const MIN_COEFFICIENT: u64 = 0x8000;

    let (new_exponent, new_coefficient) = if scaled == 0 {
        (1u32, 1u32)
    } else if scaled > 0x7FFFFF {
        let shift = 64 - scaled.leading_zeros();
        let extra_bytes = (shift.saturating_sub(23) + 7) / 8;
        let new_exp = exponent.saturating_add(extra_bytes);
        let new_coef = (scaled >> (extra_bytes * 8)) as u32;
        if new_exp > 64 {
            (64u32, 0x7FFFFFu32)
        } else {
            (new_exp, new_coef.min(0x7FFFFF))
        }
    } else if scaled < MIN_COEFFICIENT && exponent > 3 {
        // Coefficient too small - decrease exponent to maintain precision
        let new_coef = (scaled * 256).min(0x7FFFFF);
        let new_exp = exponent.saturating_sub(1);
        (new_exp, new_coef as u32)
    } else {
        (exponent, scaled as u32)
    };

    let new_bits = (new_exponent << 24) | new_coefficient;
    // Enforce minimum difficulty floor (maximum target)
    // A higher difficulty_bits value means an easier target, so we take the minimum
    new_bits.min(MIN_DIFFICULTY_BITS)
}
