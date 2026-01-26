//! Statistics collection for chaos testing.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Statistics collected during chaos testing.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct NetworkStats {
    /// When the test started.
    pub start_time: Instant,

    /// Total blocks mined across all nodes.
    pub blocks_mined: u64,

    /// Number of reorgs observed.
    pub reorg_count: u64,

    /// Maximum reorg depth observed.
    pub max_reorg_depth: u64,

    /// Distribution of reorg depths.
    pub reorg_depths: HashMap<u64, u64>,

    /// Number of node crashes.
    pub node_crashes: u64,

    /// Number of node restarts.
    pub node_restarts: u64,

    /// Number of partitions created.
    pub partitions_created: u64,

    /// Number of partitions healed.
    pub partitions_healed: u64,

    /// Number of invariant checks passed.
    pub invariant_checks_passed: u64,

    /// Number of invariant checks failed.
    pub invariant_checks_failed: u64,

    /// Invariant failure messages.
    pub invariant_failures: Vec<String>,

    /// Highest chain height observed.
    pub max_height: u64,

    /// Time spent in partitioned state.
    pub partition_duration: Duration,

    /// Number of times consensus was achieved after disruption.
    pub consensus_recoveries: u64,
}

impl NetworkStats {
    /// Create new empty stats.
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            blocks_mined: 0,
            reorg_count: 0,
            max_reorg_depth: 0,
            reorg_depths: HashMap::new(),
            node_crashes: 0,
            node_restarts: 0,
            partitions_created: 0,
            partitions_healed: 0,
            invariant_checks_passed: 0,
            invariant_checks_failed: 0,
            invariant_failures: Vec::new(),
            max_height: 0,
            partition_duration: Duration::ZERO,
            consensus_recoveries: 0,
        }
    }

    /// Record a block being mined.
    pub fn record_block(&mut self, height: u64) {
        self.blocks_mined += 1;
        if height > self.max_height {
            self.max_height = height;
        }
    }

    /// Record a reorg.
    pub fn record_reorg(&mut self, depth: u64) {
        self.reorg_count += 1;
        if depth > self.max_reorg_depth {
            self.max_reorg_depth = depth;
        }
        *self.reorg_depths.entry(depth).or_insert(0) += 1;
    }

    /// Record a node crash.
    pub fn record_crash(&mut self) {
        self.node_crashes += 1;
    }

    /// Record a node restart.
    pub fn record_restart(&mut self) {
        self.node_restarts += 1;
    }

    /// Record a partition.
    pub fn record_partition(&mut self) {
        self.partitions_created += 1;
    }

    /// Record partition healing.
    pub fn record_heal(&mut self) {
        self.partitions_healed += 1;
    }

    /// Record an invariant check result.
    pub fn record_invariant_check(&mut self, passed: bool, message: Option<String>) {
        if passed {
            self.invariant_checks_passed += 1;
        } else {
            self.invariant_checks_failed += 1;
            if let Some(msg) = message {
                self.invariant_failures.push(msg);
            }
        }
    }

    /// Record consensus recovery.
    pub fn record_consensus_recovery(&mut self) {
        self.consensus_recoveries += 1;
    }

    /// Get elapsed time.
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Generate a summary report.
    pub fn summary(&self) -> String {
        let mut report = String::new();
        report.push_str("╔══════════════════════════════════════════════════════════╗\n");
        report.push_str("║               CHAOS TEST SUMMARY                         ║\n");
        report.push_str("╠══════════════════════════════════════════════════════════╣\n");
        report.push_str(&format!(
            "║ Duration:              {:>10.1?}\n",
            self.elapsed()
        ));
        report.push_str(&format!(
            "║ Max height reached:    {:>10}\n",
            self.max_height
        ));
        report.push_str(&format!(
            "║ Blocks mined:          {:>10}\n",
            self.blocks_mined
        ));
        report.push_str("╠══════════════════════════════════════════════════════════╣\n");
        report.push_str("║ CHAOS EVENTS                                             ║\n");
        report.push_str(&format!(
            "║ Node crashes:          {:>10}\n",
            self.node_crashes
        ));
        report.push_str(&format!(
            "║ Node restarts:         {:>10}\n",
            self.node_restarts
        ));
        report.push_str(&format!(
            "║ Partitions created:    {:>10}\n",
            self.partitions_created
        ));
        report.push_str(&format!(
            "║ Partitions healed:     {:>10}\n",
            self.partitions_healed
        ));
        report.push_str("╠══════════════════════════════════════════════════════════╣\n");
        report.push_str("║ REORGS                                                   ║\n");
        report.push_str(&format!(
            "║ Total reorgs:          {:>10}\n",
            self.reorg_count
        ));
        report.push_str(&format!(
            "║ Max reorg depth:       {:>10}\n",
            self.max_reorg_depth
        ));
        if !self.reorg_depths.is_empty() {
            report.push_str("║ Reorg depth distribution:\n");
            let mut depths: Vec<_> = self.reorg_depths.iter().collect();
            depths.sort_by_key(|(d, _)| *d);
            for (depth, count) in depths {
                report.push_str(&format!("║   depth {depth}: {count:>5} times\n"));
            }
        }
        report.push_str("╠══════════════════════════════════════════════════════════╣\n");
        report.push_str("║ INVARIANTS                                               ║\n");
        report.push_str(&format!(
            "║ Checks passed:         {:>10}\n",
            self.invariant_checks_passed
        ));
        report.push_str(&format!(
            "║ Checks failed:         {:>10}\n",
            self.invariant_checks_failed
        ));
        report.push_str(&format!(
            "║ Consensus recoveries:  {:>10}\n",
            self.consensus_recoveries
        ));
        if !self.invariant_failures.is_empty() {
            report.push_str("║ Failures:\n");
            for (i, msg) in self.invariant_failures.iter().take(5).enumerate() {
                report.push_str(&format!("║   {}: {}\n", i + 1, msg));
            }
            if self.invariant_failures.len() > 5 {
                report.push_str(&format!(
                    "║   ... and {} more\n",
                    self.invariant_failures.len() - 5
                ));
            }
        }
        report.push_str("╚══════════════════════════════════════════════════════════╝\n");
        report
    }

    /// Check if the test was successful (no invariant failures).
    pub fn is_successful(&self) -> bool {
        self.invariant_checks_failed == 0
    }
}

impl Default for NetworkStats {
    fn default() -> Self {
        Self::new()
    }
}
