//! Chaos events that can be injected into the network.

use std::collections::HashSet;

/// Events that can be injected during chaos testing.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum ChaosEvent {
    /// Crash a specific node.
    CrashNode { node_id: usize },

    /// Restart a previously crashed node.
    RestartNode { node_id: usize },

    /// Create a network partition between two groups of nodes.
    Partition {
        /// Nodes in partition A.
        group_a: HashSet<usize>,
        /// Nodes in partition B.
        group_b: HashSet<usize>,
    },

    /// Heal all network partitions.
    HealPartitions,

    /// A block was mined by a node.
    BlockMined { node_id: usize, height: u64 },

    /// A reorg occurred on a node.
    Reorg {
        node_id: usize,
        old_tip: pqcoin::crypto::Hash,
        new_tip: pqcoin::crypto::Hash,
        depth: u64,
    },

    /// Invariant check completed.
    InvariantCheck { passed: bool, message: String },
}

impl std::fmt::Display for ChaosEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChaosEvent::CrashNode { node_id } => {
                write!(f, "CRASH node {node_id}")
            }
            ChaosEvent::RestartNode { node_id } => {
                write!(f, "RESTART node {node_id}")
            }
            ChaosEvent::Partition { group_a, group_b } => {
                write!(f, "PARTITION {group_a:?} | {group_b:?}")
            }
            ChaosEvent::HealPartitions => {
                write!(f, "HEAL partitions")
            }
            ChaosEvent::BlockMined { node_id, height } => {
                write!(f, "MINED block at height {height} by node {node_id}")
            }
            ChaosEvent::Reorg { node_id, depth, .. } => {
                write!(f, "REORG depth {depth} on node {node_id}")
            }
            ChaosEvent::InvariantCheck { passed, message } => {
                if *passed {
                    write!(f, "INVARIANT OK: {message}")
                } else {
                    write!(f, "INVARIANT FAILED: {message}")
                }
            }
        }
    }
}
