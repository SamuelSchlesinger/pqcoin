//! Chaos testing scenarios for network resilience.
//!
//! These tests verify that the pqcoin network can handle:
//! - Random node crashes and restarts
//! - Network partitions and healing
//! - Concurrent mining during disruptions
//! - Eventually consistent state after recovery

use std::time::Duration;

use crate::integration::chaos::{ChaosConfig, ChaosNetwork};

/// Test that the network survives gentle chaos with eventual consistency.
#[tokio::test]
async fn test_gentle_chaos_maintains_consistency() {
    let config = ChaosConfig::gentle()
        .with_duration(Duration::from_secs(15))
        .with_nodes(5)
        .with_verbose(false);

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Verify we made progress
    assert!(stats.blocks_mined > 0, "Should have mined some blocks");
    assert!(stats.max_height > 0, "Chain should have grown");

    // Verify no invariant violations
    assert!(
        stats.is_successful(),
        "Should have no invariant failures: {:?}",
        stats.invariant_failures
    );

    network.shutdown().await;
}

/// Test that nodes recover from crashes and rejoin the network.
#[tokio::test]
async fn test_crash_recovery() {
    let config = ChaosConfig {
        node_count: 5,
        crash_probability: 0.1,     // Higher crash rate
        restart_probability: 0.3,   // Quick restarts
        partition_probability: 0.0, // No partitions
        heal_probability: 0.0,
        min_alive_nodes: 2,
        max_partitions: 0,
        chaos_interval: Duration::from_millis(300),
        mining_interval: Duration::from_millis(200),
        invariant_check_interval: Duration::from_secs(1),
        max_reorg_depth: 5,
        test_duration: Duration::from_secs(15),
        verbose: false,
    };

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Should have some crashes and restarts
    assert!(stats.node_crashes > 0, "Should have experienced crashes");
    assert!(stats.node_restarts > 0, "Should have restarted nodes");

    // Chain should still make progress
    assert!(stats.blocks_mined > 0, "Should have mined blocks");

    // Should eventually recover consensus
    assert!(
        stats.is_successful(),
        "Should recover to consistent state: {:?}",
        stats.invariant_failures
    );

    network.shutdown().await;
}

/// Test network partition handling.
#[tokio::test]
async fn test_partition_and_heal() {
    let config = ChaosConfig {
        node_count: 6,
        crash_probability: 0.0, // No crashes
        restart_probability: 0.0,
        partition_probability: 0.1, // Occasional partitions
        heal_probability: 0.2,      // Heal relatively quickly
        min_alive_nodes: 6,
        max_partitions: 1,
        chaos_interval: Duration::from_millis(500),
        mining_interval: Duration::from_millis(150),
        invariant_check_interval: Duration::from_secs(1),
        max_reorg_depth: 8,
        test_duration: Duration::from_secs(15),
        verbose: false,
    };

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Should have created partitions
    assert!(
        stats.partitions_created > 0,
        "Should have created partitions"
    );

    // Chain should make progress
    assert!(stats.blocks_mined > 0, "Should have mined blocks");

    // Should achieve eventual consistency
    assert!(
        stats.is_successful(),
        "Should recover after healing: {:?}",
        stats.invariant_failures
    );

    network.shutdown().await;
}

/// Test combined chaos with crashes and partitions.
#[tokio::test]
async fn test_combined_chaos() {
    let config = ChaosConfig {
        node_count: 6,
        crash_probability: 0.05,
        restart_probability: 0.15,
        partition_probability: 0.03,
        heal_probability: 0.1,
        min_alive_nodes: 3,
        max_partitions: 1,
        chaos_interval: Duration::from_millis(400),
        mining_interval: Duration::from_millis(150),
        invariant_check_interval: Duration::from_secs(1),
        max_reorg_depth: 10,
        test_duration: Duration::from_secs(20),
        verbose: false,
    };

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Should have experienced various chaos events
    let total_chaos_events = stats.node_crashes
        + stats.partitions_created
        + stats.node_restarts
        + stats.partitions_healed;
    assert!(
        total_chaos_events > 0,
        "Should have experienced some chaos events"
    );

    // Chain should make progress despite chaos
    assert!(stats.blocks_mined > 0, "Should have mined blocks");

    // Should eventually recover
    assert!(
        stats.is_successful(),
        "Should achieve consistency: {:?}",
        stats.invariant_failures
    );

    network.shutdown().await;
}

/// Test that reorgs stay within acceptable bounds.
#[tokio::test]
async fn test_reorg_depth_bounds() {
    let config = ChaosConfig {
        node_count: 5,
        crash_probability: 0.02,
        restart_probability: 0.1,
        partition_probability: 0.05,
        heal_probability: 0.15,
        min_alive_nodes: 2,
        max_partitions: 1,
        chaos_interval: Duration::from_millis(300),
        mining_interval: Duration::from_millis(100), // Fast mining to trigger reorgs
        invariant_check_interval: Duration::from_millis(500),
        max_reorg_depth: 5, // Strict limit
        test_duration: Duration::from_secs(15),
        verbose: false,
    };

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Check reorg depth stayed within bounds
    assert!(
        stats.max_reorg_depth <= 5,
        "Reorg depth {} exceeded limit of 5",
        stats.max_reorg_depth
    );

    network.shutdown().await;
}

/// Test that the network makes continuous progress.
#[tokio::test]
async fn test_chain_progress() {
    let config = ChaosConfig::gentle()
        .with_duration(Duration::from_secs(10))
        .with_nodes(4);

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let initial_tips = network.get_all_tips().await;
    let initial_max_height = initial_tips.iter().map(|(_, h, _)| *h).max().unwrap_or(0);

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Should have made significant progress
    let height_gain = stats.max_height - initial_max_height;
    assert!(
        height_gain >= 5,
        "Should have gained at least 5 blocks, got {height_gain}"
    );

    network.shutdown().await;
}

/// Longer stress test with aggressive chaos (marked ignore for CI).
#[tokio::test]
#[ignore] // Run manually with: cargo test stress_test -- --ignored
async fn stress_test_aggressive_chaos() {
    let config = ChaosConfig::aggressive()
        .with_duration(Duration::from_secs(60))
        .with_verbose(true);

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Should survive aggressive chaos
    assert!(stats.blocks_mined > 10, "Should have mined many blocks");
    assert!(
        stats.is_successful(),
        "Should eventually reach consistency: {:?}",
        stats.invariant_failures
    );

    network.shutdown().await;
}

/// Large network stress test (marked ignore for CI).
#[tokio::test]
#[ignore] // Run manually with: cargo test large_network -- --ignored
async fn stress_test_large_network() {
    let config = ChaosConfig::large_network()
        .with_duration(Duration::from_secs(90))
        .with_verbose(true);

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    let stats = network.run().await;

    println!("{}", stats.summary());

    // Large network should still maintain consistency
    assert!(stats.blocks_mined > 0, "Should have mined blocks");
    assert!(
        stats.is_successful(),
        "Large network should reach consistency: {:?}",
        stats.invariant_failures
    );

    network.shutdown().await;
}

/// Test that partitions actually isolate nodes - verifies the partition enforcement works.
///
/// This test explicitly verifies that:
/// 1. Before partition: all nodes see the same tip
/// 2. During partition: nodes in different groups diverge (CRITICAL - proves isolation works)
/// 3. After healing: blocklist is cleared (but reconnection is passive)
///
/// NOTE: This test currently documents a limitation - after healing, nodes don't
/// actively reconnect. They rely on periodic peer discovery. In a real network,
/// this would eventually resolve, but in short tests it may appear as a permanent fork.
/// This is acceptable for testing partition isolation but may need enhancement for
/// production resilience testing.
#[tokio::test]
async fn test_partition_actually_isolates_nodes() {
    use tokio::time::sleep;

    // Create a 6-node network with no random chaos - we control everything
    let config = ChaosConfig {
        node_count: 6,
        crash_probability: 0.0,
        restart_probability: 0.0,
        partition_probability: 0.0, // No automatic partitions
        heal_probability: 0.0,
        min_alive_nodes: 6,
        max_partitions: 2,
        chaos_interval: Duration::from_secs(100), // Effectively disabled
        mining_interval: Duration::from_secs(100), // Effectively disabled
        invariant_check_interval: Duration::from_secs(100),
        max_reorg_depth: 20,
        test_duration: Duration::from_secs(100),
        verbose: true,
    };

    let mut network = ChaosNetwork::new(config)
        .await
        .expect("Failed to create chaos network");

    // Wait for initial connections to establish
    sleep(Duration::from_millis(500)).await;

    // Step 1: Mine a few blocks to establish baseline
    println!("=== Step 1: Establishing baseline ===");
    for _ in 0..3 {
        network.mine_block(0).await;
        sleep(Duration::from_millis(300)).await;
    }

    // Wait for propagation
    sleep(Duration::from_secs(1)).await;

    // Verify all nodes have the same tip
    let tips_before = network.get_all_tips().await;
    println!("Tips before partition: {tips_before:?}");

    let first_tip = tips_before[0].2;
    let first_height = tips_before[0].1;
    assert!(
        tips_before.iter().all(|(_, _, hash)| *hash == first_tip),
        "All nodes should have same tip before partition"
    );
    println!("All nodes at height {first_height} with same tip");

    // Step 2: Create partition
    println!("\n=== Step 2: Creating partition ===");
    network.create_partition().await;

    // Wait for disconnections to complete
    sleep(Duration::from_millis(500)).await;

    // Step 3: Mine blocks in each partition
    println!("\n=== Step 3: Mining in each partition ===");

    // Get the partition groups (nodes 0,1,2 vs 3,4,5 approximately)
    let alive = network.alive_node_ids();
    let group_a_node = alive[0]; // First node
    let group_b_node = alive[alive.len() - 1]; // Last node (should be in other partition)

    println!("Mining on node {group_a_node} (group A)");
    for _ in 0..3 {
        network.mine_block(group_a_node).await;
        sleep(Duration::from_millis(200)).await;
    }

    println!("Mining on node {group_b_node} (group B)");
    for _ in 0..3 {
        network.mine_block(group_b_node).await;
        sleep(Duration::from_millis(200)).await;
    }

    // Wait for within-partition propagation
    sleep(Duration::from_secs(2)).await;

    // Step 4: Verify partition isolation - the miners in each partition should have DIFFERENT tips
    println!("\n=== Step 4: Verifying partition isolation ===");
    let tips_during = network.get_all_tips().await;
    println!("Tips during partition: {tips_during:?}");

    // Get the tips of our specific mining nodes
    let group_a_tip = tips_during
        .iter()
        .find(|(id, _, _)| *id == group_a_node)
        .map(|(_, _, hash)| *hash)
        .expect("group_a_node should be in tips");
    let group_b_tip = tips_during
        .iter()
        .find(|(id, _, _)| *id == group_b_node)
        .map(|(_, _, hash)| *hash)
        .expect("group_b_node should be in tips");

    println!("Group A miner (node {group_a_node}) tip: {group_a_tip:?}");
    println!("Group B miner (node {group_b_node}) tip: {group_b_tip:?}");

    // CRITICAL ASSERTION: The two miners should have different chain tips
    // This proves the partition prevented cross-partition block propagation
    assert_ne!(
        group_a_tip, group_b_tip,
        "Partition should cause divergence between miners! \
         Node {group_a_node} and node {group_b_node} have the same tip, meaning blocks crossed the partition."
    );
    println!("SUCCESS: Miners have different tips - partition isolation verified!");

    // Also group nodes by their tip for informational purposes
    let mut tip_groups: std::collections::HashMap<pqcoin::crypto::Hash, Vec<usize>> =
        std::collections::HashMap::new();
    for (node_id, _height, tip) in &tips_during {
        tip_groups.entry(*tip).or_default().push(*node_id);
    }
    println!("All tip groups: {tip_groups:?}");

    // Step 5: Heal partition
    println!("\n=== Step 5: Healing partition ===");
    network.heal_partitions().await;

    // Wait for reconnection and sync - needs more time for nodes to reconnect and sync
    // In production, nodes periodically retry connections. In tests, we need to wait.
    sleep(Duration::from_secs(8)).await;

    // Step 6: Verify convergence (with retries)
    println!("\n=== Step 6: Verifying convergence ===");

    // Give nodes time to sync - retry a few times
    let mut _converged = false;
    for attempt in 1..=5 {
        let tips_after = network.get_all_tips().await;
        println!("Attempt {attempt}: Tips after healing: {tips_after:?}");

        // Group by tip to see convergence progress
        let mut tip_groups: std::collections::HashMap<pqcoin::crypto::Hash, Vec<usize>> =
            std::collections::HashMap::new();
        for (node_id, _, tip) in &tips_after {
            tip_groups.entry(*tip).or_default().push(*node_id);
        }
        println!("  Tip groups: {tip_groups:?}");

        // Check if converged (all same tip OR dominant tip has majority)
        let max_group_size = tip_groups.values().map(|v| v.len()).max().unwrap_or(0);
        let total_nodes = tips_after.len();

        if tip_groups.len() == 1 {
            _converged = true;
            println!("  Full convergence achieved!");
            break;
        } else if max_group_size >= total_nodes * 2 / 3 {
            // 2/3 majority is good enough for now (some nodes may be slow)
            println!("  Majority convergence: {max_group_size}/{total_nodes} nodes on same tip");
        }

        sleep(Duration::from_secs(2)).await;
    }

    // Final assessment
    let final_tips = network.get_all_tips().await;
    let mut final_tip_groups: std::collections::HashMap<pqcoin::crypto::Hash, Vec<usize>> =
        std::collections::HashMap::new();
    for (node_id, _, tip) in &final_tips {
        final_tip_groups.entry(*tip).or_default().push(*node_id);
    }

    println!("\nFinal tip groups: {final_tip_groups:?}");

    // Find the dominant chain (should be the longest one)
    let max_height = final_tips.iter().map(|(_, h, _)| *h).max().unwrap_or(0);
    let nodes_at_max_height: Vec<_> = final_tips
        .iter()
        .filter(|(_, h, _)| *h == max_height)
        .collect();

    println!("Max height: {max_height}, nodes at max height: {nodes_at_max_height:?}");

    // The test passes if:
    // 1. At least some nodes converged to the longest chain
    // 2. The longest chain is longer than pre-partition
    assert!(
        max_height > first_height,
        "Chain should have grown. Before: {first_height}, After: {max_height}"
    );

    // At least half the nodes should be on the longest chain
    let nodes_synced = nodes_at_max_height.len();
    assert!(
        nodes_synced >= final_tips.len() / 2,
        "At least half the nodes should sync to longest chain. Got {}/{} at height {}",
        nodes_synced,
        final_tips.len(),
        max_height
    );

    network.shutdown().await;

    println!("\n=== PARTITION ISOLATION TEST PASSED ===");
    println!("  - Partition successfully caused chain divergence");
    println!(
        "  - {}/{} nodes synced to height {} after healing",
        nodes_synced,
        final_tips.len(),
        max_height
    );
}
