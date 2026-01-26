//! Chaos testing configuration.

use std::time::Duration;

/// Configuration for chaos testing behavior.
#[derive(Debug, Clone)]
pub struct ChaosConfig {
    /// Number of nodes in the network.
    pub node_count: usize,

    /// Probability of a node crash per chaos tick (0.0 - 1.0).
    pub crash_probability: f64,

    /// Probability of a network partition per chaos tick (0.0 - 1.0).
    pub partition_probability: f64,

    /// Probability of healing a partition per chaos tick (0.0 - 1.0).
    pub heal_probability: f64,

    /// Probability of restarting a crashed node per chaos tick (0.0 - 1.0).
    pub restart_probability: f64,

    /// Minimum nodes that must stay alive.
    pub min_alive_nodes: usize,

    /// Maximum simultaneous partitions.
    pub max_partitions: usize,

    /// How often to inject chaos events.
    pub chaos_interval: Duration,

    /// How often to mine blocks (per node that's mining).
    pub mining_interval: Duration,

    /// How often to check invariants.
    pub invariant_check_interval: Duration,

    /// Maximum allowed reorg depth before flagging.
    pub max_reorg_depth: u64,

    /// Total duration to run the chaos test.
    pub test_duration: Duration,

    /// Whether to enable verbose logging.
    pub verbose: bool,
}

impl Default for ChaosConfig {
    fn default() -> Self {
        Self {
            node_count: 10,
            crash_probability: 0.05,
            partition_probability: 0.02,
            heal_probability: 0.1,
            restart_probability: 0.2,
            min_alive_nodes: 3,
            max_partitions: 2,
            chaos_interval: Duration::from_millis(500),
            mining_interval: Duration::from_millis(200),
            invariant_check_interval: Duration::from_secs(1),
            max_reorg_depth: 10,
            test_duration: Duration::from_secs(60),
            verbose: false,
        }
    }
}

impl ChaosConfig {
    /// Create a gentle chaos config for shorter tests.
    pub fn gentle() -> Self {
        Self {
            node_count: 5,
            crash_probability: 0.02,
            partition_probability: 0.01,
            heal_probability: 0.2,
            restart_probability: 0.3,
            min_alive_nodes: 2,
            max_partitions: 1,
            chaos_interval: Duration::from_millis(500),
            mining_interval: Duration::from_millis(100),
            invariant_check_interval: Duration::from_secs(1),
            max_reorg_depth: 5,
            test_duration: Duration::from_secs(30),
            verbose: false,
        }
    }

    /// Create an aggressive chaos config for stress testing.
    pub fn aggressive() -> Self {
        Self {
            node_count: 15,
            crash_probability: 0.1,
            partition_probability: 0.05,
            heal_probability: 0.15,
            restart_probability: 0.25,
            min_alive_nodes: 3,
            max_partitions: 3,
            chaos_interval: Duration::from_millis(300),
            mining_interval: Duration::from_millis(100),
            invariant_check_interval: Duration::from_millis(500),
            max_reorg_depth: 15,
            test_duration: Duration::from_secs(120),
            verbose: false,
        }
    }

    /// Create config for a large network test.
    pub fn large_network() -> Self {
        Self {
            node_count: 30,
            crash_probability: 0.03,
            partition_probability: 0.02,
            heal_probability: 0.1,
            restart_probability: 0.15,
            min_alive_nodes: 10,
            max_partitions: 4,
            chaos_interval: Duration::from_millis(500),
            mining_interval: Duration::from_millis(150),
            invariant_check_interval: Duration::from_secs(2),
            max_reorg_depth: 10,
            test_duration: Duration::from_secs(180),
            verbose: false,
        }
    }

    /// Enable verbose logging.
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Set test duration.
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.test_duration = duration;
        self
    }

    /// Set node count.
    pub fn with_nodes(mut self, count: usize) -> Self {
        self.node_count = count;
        self
    }
}
