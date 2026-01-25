//! Dust limit calculation.

use crate::constants::DUST_FEE_RATE;
use super::locking::LockingCondition;
use super::output::TxOutput;

/// Calculate the dust limit for a given locking condition.
///
/// The dust limit is based on the cost to spend the output at DUST_FEE_RATE.
/// This ensures outputs are economically rational to spend while allowing
/// small payments (since DUST_FEE_RATE is lower than MIN_RELAY_FEE).
pub(crate) fn dust_limit(condition: &LockingCondition) -> u64 {
    let spend_size = condition.estimated_spend_size() as u64;
    // dust = spend_size * DUST_FEE_RATE / 1000 (rate is per KB)
    (spend_size * DUST_FEE_RATE) / 1000
}

/// Check if an output amount is below the dust limit for its locking condition.
/// Dust outputs are uneconomical to spend and bloat the UTXO set.
pub(crate) fn is_dust(output: &TxOutput) -> bool {
    output.amount < dust_limit(&output.condition)
}
