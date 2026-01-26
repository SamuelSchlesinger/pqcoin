//! ChaosNetwork - A network wrapper that injects failures for resilience testing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use pqcoin::blockchain::{Address, Block};
use pqcoin::crypto::{Hash, ml_dsa_87};
use rand::Rng;
use tokio::sync::RwLock;
use tokio::time::sleep;

use super::config::ChaosConfig;
use super::events::ChaosEvent;
use super::invariants::{InvariantViolation, group_by_tip, tips_are_consistent};
use super::stats::NetworkStats;

use crate::integration::helpers::create_test_genesis;
use crate::integration::test_network::find_available_ports;
use crate::integration::test_node::{RestartInfo, TestNode};

/// State of a node in the chaos network.
enum NodeState {
    /// Node is running.
    Alive(TestNode),
    /// Node is crashed, holding restart info.
    Crashed(RestartInfo),
    /// Node is being restarted (transitional state).
    Restarting,
}

/// A network that supports chaos injection for resilience testing.
#[allow(dead_code)]
pub struct ChaosNetwork {
    /// Configuration for chaos behavior.
    config: ChaosConfig,

    /// Node states (alive, crashed, or restarting).
    nodes: HashMap<usize, NodeState>,

    /// Genesis block shared by all nodes.
    genesis: Block,

    /// Miner address for genesis.
    miner_address: Address,

    /// Currently active network partitions.
    /// Each partition is a set of node IDs that can communicate with each other.
    partitions: Vec<HashSet<usize>>,

    /// Statistics collected during the test.
    stats: Arc<RwLock<NetworkStats>>,

    /// Event log.
    events: Arc<RwLock<Vec<(Instant, ChaosEvent)>>>,

    /// Previous tips for detecting reorgs.
    previous_tips: HashMap<usize, (u64, Hash)>,

    /// Whether partitions are currently active.
    is_partitioned: bool,

    /// Last time consensus was checked.
    last_consensus_check: Instant,

    /// Whether nodes were in consensus at last check.
    was_in_consensus: bool,
}

impl ChaosNetwork {
    /// Create a new chaos network with the given configuration.
    pub async fn new(
        config: ChaosConfig,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (pk, _) = ml_dsa_87::keygen();
        let miner_address = Address::from_public_key(&pk);
        let genesis = create_test_genesis(miner_address);

        // Find ports for all nodes
        let ports = find_available_ports(config.node_count).await?;

        // Build seed peers (full mesh initially)
        let mut nodes = HashMap::new();
        let mut all_addrs: Vec<std::net::SocketAddr> = Vec::new();

        // Create nodes one by one, each connecting to all previous nodes
        for (id, port) in ports.iter().enumerate() {
            let seed_peers = all_addrs.clone();
            let node = TestNode::create_with_port(id, *port, seed_peers, genesis.clone()).await?;
            all_addrs.push(node.addr());
            nodes.insert(id, NodeState::Alive(node));
        }

        // Wait for initial connections
        sleep(Duration::from_millis(500)).await;

        Ok(Self {
            config,
            nodes,
            genesis,
            miner_address,
            partitions: Vec::new(),
            stats: Arc::new(RwLock::new(NetworkStats::new())),
            events: Arc::new(RwLock::new(Vec::new())),
            previous_tips: HashMap::new(),
            is_partitioned: false,
            last_consensus_check: Instant::now(),
            was_in_consensus: true,
        })
    }

    /// Get the number of alive nodes.
    #[allow(dead_code)]
    pub fn alive_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|s| matches!(s, NodeState::Alive(_)))
            .count()
    }

    /// Get IDs of alive nodes.
    pub fn alive_node_ids(&self) -> Vec<usize> {
        self.nodes
            .iter()
            .filter_map(|(id, s)| {
                if matches!(s, NodeState::Alive(_)) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get IDs of crashed nodes.
    pub fn crashed_node_ids(&self) -> Vec<usize> {
        self.nodes
            .iter()
            .filter_map(|(id, s)| {
                if matches!(s, NodeState::Crashed(_)) {
                    Some(*id)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Log an event.
    async fn log_event(&self, event: ChaosEvent) {
        if self.config.verbose {
            println!("[{:?}] {}", self.stats.read().await.elapsed(), event);
        }
        self.events.write().await.push((Instant::now(), event));
    }

    /// Crash a random node (if possible).
    pub async fn crash_random_node(&mut self) -> Option<usize> {
        let alive = self.alive_node_ids();
        if alive.len() <= self.config.min_alive_nodes {
            return None;
        }

        let mut rng = rand::thread_rng();
        let idx = rng.gen_range(0..alive.len());
        let node_id = alive[idx];

        self.crash_node(node_id).await
    }

    /// Crash a specific node.
    pub async fn crash_node(&mut self, node_id: usize) -> Option<usize> {
        if let Some(NodeState::Alive(node)) = self.nodes.remove(&node_id) {
            let restart_info = node.shutdown_for_restart().await;
            self.nodes.insert(node_id, NodeState::Crashed(restart_info));
            self.stats.write().await.record_crash();
            self.log_event(ChaosEvent::CrashNode { node_id }).await;
            Some(node_id)
        } else {
            None
        }
    }

    /// Restart a random crashed node.
    pub async fn restart_random_node(&mut self) -> Option<usize> {
        let crashed = self.crashed_node_ids();
        if crashed.is_empty() {
            return None;
        }

        let mut rng = rand::thread_rng();
        let idx = rng.gen_range(0..crashed.len());
        let node_id = crashed[idx];

        self.restart_node(node_id).await
    }

    /// Restart a specific crashed node.
    pub async fn restart_node(&mut self, node_id: usize) -> Option<usize> {
        if let Some(NodeState::Crashed(restart_info)) = self.nodes.remove(&node_id) {
            self.nodes.insert(node_id, NodeState::Restarting);

            // Find a new port
            let ports = match find_available_ports(1).await {
                Ok(p) => p,
                Err(_) => {
                    self.nodes.insert(node_id, NodeState::Crashed(restart_info));
                    return None;
                }
            };

            // Get seed peers from alive nodes
            let seed_peers: Vec<_> = self
                .nodes
                .iter()
                .filter_map(|(_, state)| {
                    if let NodeState::Alive(node) = state {
                        Some(node.addr())
                    } else {
                        None
                    }
                })
                .collect();

            match restart_info
                .restart(ports[0], seed_peers, self.genesis.clone())
                .await
            {
                Ok(node) => {
                    self.nodes.insert(node_id, NodeState::Alive(node));
                    self.stats.write().await.record_restart();
                    self.log_event(ChaosEvent::RestartNode { node_id }).await;
                    Some(node_id)
                }
                Err(_) => {
                    self.nodes.remove(&node_id);
                    None
                }
            }
        } else {
            None
        }
    }

    /// Create a network partition.
    pub async fn create_partition(&mut self) {
        if self.partitions.len() >= self.config.max_partitions {
            return;
        }

        let alive = self.alive_node_ids();
        if alive.len() < 4 {
            return; // Need at least 4 nodes for meaningful partition
        }

        let mut rng = rand::thread_rng();
        let split_point = rng.gen_range(1..alive.len());

        let group_a: HashSet<_> = alive[..split_point].iter().copied().collect();
        let group_b: HashSet<_> = alive[split_point..].iter().copied().collect();

        // Collect addresses for each partition group
        let mut group_a_addrs: Vec<std::net::SocketAddr> = Vec::new();
        let mut group_b_addrs: Vec<std::net::SocketAddr> = Vec::new();

        for &node_id in &group_a {
            if let Some(NodeState::Alive(node)) = self.nodes.get(&node_id) {
                group_a_addrs.push(node.addr());
            }
        }
        for &node_id in &group_b {
            if let Some(NodeState::Alive(node)) = self.nodes.get(&node_id) {
                group_b_addrs.push(node.addr());
            }
        }

        // For each node in group A: block all group B addresses and disconnect
        for &node_id in &group_a {
            if let Some(NodeState::Alive(node)) = self.nodes.get(&node_id) {
                // Block all group B addresses
                for addr in &group_b_addrs {
                    node.block_addr(*addr).await;
                }
                // Disconnect from all group B addresses
                for addr in &group_b_addrs {
                    node.disconnect_addr(addr).await;
                }
            }
        }

        // For each node in group B: block all group A addresses and disconnect
        for &node_id in &group_b {
            if let Some(NodeState::Alive(node)) = self.nodes.get(&node_id) {
                // Block all group A addresses
                for addr in &group_a_addrs {
                    node.block_addr(*addr).await;
                }
                // Disconnect from all group A addresses
                for addr in &group_a_addrs {
                    node.disconnect_addr(addr).await;
                }
            }
        }

        self.partitions.push(group_a.clone());
        self.partitions.push(group_b.clone());
        self.is_partitioned = true;

        self.stats.write().await.record_partition();
        self.log_event(ChaosEvent::Partition { group_a, group_b })
            .await;
    }

    /// Heal all partitions.
    ///
    /// This clears the blocklist on all nodes AND forces reconnection by
    /// cycling one node (crash + restart). This ensures partitioned nodes
    /// actually reconnect rather than staying isolated.
    pub async fn heal_partitions(&mut self) {
        if !self.partitions.is_empty() {
            // Clear blocklist on all alive nodes
            for state in self.nodes.values() {
                if let NodeState::Alive(node) = state {
                    node.clear_blocklist().await;
                }
            }

            // Force reconnection by cycling a random node
            // When the node restarts, it connects to all alive nodes as seed peers,
            // which bridges the partitions
            let alive = self.alive_node_ids();
            if alive.len() >= 2 {
                // Pick a node to cycle (preferably from the larger partition)
                let cycle_node = alive[0];
                if let Some(NodeState::Alive(node)) = self.nodes.remove(&cycle_node) {
                    let restart_info = node.shutdown_for_restart().await;

                    // Find a new port
                    if let Ok(ports) = find_available_ports(1).await {
                        // Get seed peers from all remaining alive nodes
                        let seed_peers: Vec<_> = self
                            .nodes
                            .iter()
                            .filter_map(|(_, state)| {
                                if let NodeState::Alive(node) = state {
                                    Some(node.addr())
                                } else {
                                    None
                                }
                            })
                            .collect();

                        if let Ok(node) = restart_info
                            .restart(ports[0], seed_peers, self.genesis.clone())
                            .await
                        {
                            self.nodes.insert(cycle_node, NodeState::Alive(node));
                            tracing::debug!(
                                node_id = cycle_node,
                                "cycled node to bridge partitions"
                            );
                        }
                    }
                }
            }

            self.partitions.clear();
            self.is_partitioned = false;
            self.stats.write().await.record_heal();
            self.log_event(ChaosEvent::HealPartitions).await;
        }
    }

    /// Mine a block on a random alive node.
    pub async fn mine_random_block(&mut self) -> Option<(usize, Block)> {
        let alive = self.alive_node_ids();
        if alive.is_empty() {
            return None;
        }

        let mut rng = rand::thread_rng();
        let idx = rng.gen_range(0..alive.len());
        let node_id = alive[idx];

        self.mine_block(node_id).await.map(|b| (node_id, b))
    }

    /// Mine a block on a specific node.
    pub async fn mine_block(&mut self, node_id: usize) -> Option<Block> {
        if let Some(NodeState::Alive(node)) = self.nodes.get(&node_id) {
            if let Some(block) = node.mine_and_submit_block().await {
                let height = node.height().await;
                self.stats.write().await.record_block(height);
                self.log_event(ChaosEvent::BlockMined { node_id, height })
                    .await;

                // Check for reorgs
                self.check_for_reorg(node_id).await;

                return Some(block);
            }
        }
        None
    }

    /// Check if a reorg occurred on a node.
    async fn check_for_reorg(&mut self, node_id: usize) {
        if let Some(NodeState::Alive(node)) = self.nodes.get(&node_id) {
            let current_height = node.height().await;
            let current_tip = node.tip_hash().await;

            if let Some((prev_height, prev_tip)) = self.previous_tips.get(&node_id) {
                // A reorg happened if:
                // 1. The tip changed
                // 2. But the new height isn't just prev_height + 1 with new tip being a child
                // For simplicity, we detect reorg as tip change where height decreased or stayed same
                if current_tip != *prev_tip && current_height <= *prev_height {
                    let depth = prev_height - current_height + 1;
                    self.stats.write().await.record_reorg(depth);
                    self.log_event(ChaosEvent::Reorg {
                        node_id,
                        old_tip: *prev_tip,
                        new_tip: current_tip,
                        depth,
                    })
                    .await;
                }
            }

            self.previous_tips
                .insert(node_id, (current_height, current_tip));
        }
    }

    /// Get tips from all alive nodes.
    pub async fn get_all_tips(&self) -> Vec<(usize, u64, Hash)> {
        let mut tips = Vec::new();
        for (id, state) in &self.nodes {
            if let NodeState::Alive(node) = state {
                let height = node.height().await;
                let tip = node.tip_hash().await;
                tips.push((*id, height, tip));
            }
        }
        tips
    }

    /// Check if all alive nodes are in consensus.
    #[allow(dead_code)]
    pub async fn check_consensus(&self) -> bool {
        let tips = self.get_all_tips().await;
        tips_are_consistent(&tips)
    }

    /// Run invariant checks.
    pub async fn check_invariants(&mut self) -> Result<(), InvariantViolation> {
        let tips = self.get_all_tips().await;

        // Check for consensus (after allowing time for propagation if not partitioned)
        if !self.is_partitioned {
            let in_consensus = tips_are_consistent(&tips);

            if in_consensus && !self.was_in_consensus {
                // Consensus recovered
                self.stats.write().await.record_consensus_recovery();
            }

            self.was_in_consensus = in_consensus;

            // If we've been without partitions for a while, we should have consensus
            if !in_consensus && self.last_consensus_check.elapsed() > Duration::from_secs(5) {
                let groups = group_by_tip(&tips);
                let node_tips: HashMap<_, _> = tips
                    .iter()
                    .map(|(id, h, hash)| (*id, (*h, *hash)))
                    .collect();

                return Err(InvariantViolation {
                    invariant: "EventualConsistency".to_string(),
                    message: format!(
                        "Nodes split into {} groups after 5s without partitions",
                        groups.len()
                    ),
                    node_tips,
                });
            }
        }

        // Check max reorg depth
        {
            let stats = self.stats.read().await;
            if stats.max_reorg_depth > self.config.max_reorg_depth {
                return Err(InvariantViolation {
                    invariant: "MaxReorgDepth".to_string(),
                    message: format!(
                        "Reorg depth {} exceeds max {}",
                        stats.max_reorg_depth, self.config.max_reorg_depth
                    ),
                    node_tips: tips
                        .iter()
                        .map(|(id, h, hash)| (*id, (*h, *hash)))
                        .collect(),
                });
            }
        } // Drop read lock before acquiring write lock

        self.stats.write().await.record_invariant_check(true, None);
        self.log_event(ChaosEvent::InvariantCheck {
            passed: true,
            message: "All invariants passed".to_string(),
        })
        .await;

        self.last_consensus_check = Instant::now();
        Ok(())
    }

    /// Run one tick of chaos injection.
    pub async fn chaos_tick(&mut self) {
        let mut rng = rand::thread_rng();

        // Maybe crash a node
        if rng.r#gen::<f64>() < self.config.crash_probability {
            self.crash_random_node().await;
        }

        // Maybe restart a node
        if rng.r#gen::<f64>() < self.config.restart_probability {
            self.restart_random_node().await;
        }

        // Maybe create partition
        if rng.r#gen::<f64>() < self.config.partition_probability {
            self.create_partition().await;
        }

        // Maybe heal partitions
        if rng.r#gen::<f64>() < self.config.heal_probability {
            self.heal_partitions().await;
        }
    }

    /// Run the chaos test for the configured duration.
    pub async fn run(&mut self) -> NetworkStats {
        let start = Instant::now();
        let mut last_chaos = Instant::now();
        let mut last_mine = Instant::now();
        let mut last_invariant = Instant::now();

        while start.elapsed() < self.config.test_duration {
            // Chaos injection
            if last_chaos.elapsed() >= self.config.chaos_interval {
                self.chaos_tick().await;
                last_chaos = Instant::now();
            }

            // Mining
            if last_mine.elapsed() >= self.config.mining_interval {
                self.mine_random_block().await;
                last_mine = Instant::now();
            }

            // Invariant checks
            if last_invariant.elapsed() >= self.config.invariant_check_interval {
                if let Err(violation) = self.check_invariants().await {
                    self.stats
                        .write()
                        .await
                        .record_invariant_check(false, Some(violation.to_string()));
                    self.log_event(ChaosEvent::InvariantCheck {
                        passed: false,
                        message: violation.to_string(),
                    })
                    .await;
                }
                last_invariant = Instant::now();
            }

            // Small sleep to prevent busy loop
            sleep(Duration::from_millis(10)).await;
        }

        // Final invariant check after healing
        self.heal_partitions().await;
        sleep(Duration::from_secs(2)).await; // Allow time for final sync

        if let Err(violation) = self.check_invariants().await {
            self.stats
                .write()
                .await
                .record_invariant_check(false, Some(format!("Final check: {violation}")));
        }

        self.stats.read().await.clone()
    }

    /// Shutdown all nodes.
    pub async fn shutdown(self) {
        for (_, state) in self.nodes {
            match state {
                NodeState::Alive(node) => node.shutdown().await,
                NodeState::Crashed(info) => drop(info), // Will clean up storage
                NodeState::Restarting => {}
            }
        }
    }

    /// Get current stats.
    #[allow(dead_code)]
    pub async fn stats(&self) -> NetworkStats {
        self.stats.read().await.clone()
    }

    /// Get event log.
    #[allow(dead_code)]
    pub async fn events(&self) -> Vec<(Instant, ChaosEvent)> {
        self.events.read().await.clone()
    }
}
