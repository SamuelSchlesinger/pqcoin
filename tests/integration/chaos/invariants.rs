//! Invariant definitions and checking for chaos testing.

use pqcoin::crypto::Hash;
use std::collections::HashMap;

/// An invariant that should hold during chaos testing.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum Invariant {
    /// All live nodes should eventually converge to the same tip.
    EventualConsistency {
        /// Maximum allowed time for convergence after last disruption.
        timeout_secs: u64,
    },

    /// Reorg depth should not exceed a threshold.
    MaxReorgDepth { max_depth: u64 },

    /// No permanent forks (all live nodes on same chain within timeout).
    NoPermanentForks { timeout_secs: u64 },

    /// Chain should make progress (height should increase).
    ChainProgress {
        /// Minimum blocks expected per interval.
        min_blocks_per_minute: u64,
    },

    /// Restarted nodes should recover their state.
    StateRecovery,

    /// No data corruption (blocks should be valid).
    NoDataCorruption,
}

/// Result of an invariant violation.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct InvariantViolation {
    /// Which invariant was violated.
    pub invariant: String,
    /// Description of the violation.
    pub message: String,
    /// Node tips at time of violation.
    pub node_tips: HashMap<usize, (u64, Hash)>,
}

impl std::fmt::Display for InvariantViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.invariant, self.message)
    }
}

/// Check if all provided tips are consistent (same hash).
pub fn tips_are_consistent(tips: &[(usize, u64, Hash)]) -> bool {
    if tips.is_empty() {
        return true;
    }

    let first_tip = tips[0].2;
    tips.iter().all(|(_, _, hash)| *hash == first_tip)
}

/// Find the maximum height among tips.
#[allow(dead_code)]
pub fn max_height(tips: &[(usize, u64, Hash)]) -> u64 {
    tips.iter().map(|(_, h, _)| *h).max().unwrap_or(0)
}

/// Find nodes that are behind the maximum height.
#[allow(dead_code)]
pub fn nodes_behind(tips: &[(usize, u64, Hash)]) -> Vec<(usize, u64)> {
    let max_h = max_height(tips);
    tips.iter()
        .filter(|(_, h, _)| *h < max_h)
        .map(|(id, h, _)| (*id, max_h - *h))
        .collect()
}

/// Group nodes by their tip hash.
pub fn group_by_tip(tips: &[(usize, u64, Hash)]) -> HashMap<Hash, Vec<usize>> {
    let mut groups: HashMap<Hash, Vec<usize>> = HashMap::new();
    for (id, _, hash) in tips {
        groups.entry(*hash).or_default().push(*id);
    }
    groups
}
