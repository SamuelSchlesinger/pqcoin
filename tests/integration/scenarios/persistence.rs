//! Persistence and restart tests.
//!
//! These tests verify that nodes can shut down and restart from persistent
//! storage, maintaining blockchain state and rejoining the network correctly.

use std::time::Duration;
use tokio::time::sleep;

use crate::integration::helpers::{DEFAULT_TIMEOUT, create_test_genesis, wait_for_height};
use crate::integration::test_network::{TestNetwork, Topology, find_available_ports};
use crate::integration::test_node::TestNode;

use pqcoin::blockchain::Address;
use pqcoin::crypto::ml_dsa_87;

/// Test that a node restarts and recovers its blockchain state.
#[tokio::test]
async fn test_node_restart_recovers_state() {
    // Create a single node
    let (pk, _) = ml_dsa_87::keygen();
    let miner_address = Address::from_public_key(&pk);
    let genesis = create_test_genesis(miner_address);

    let ports = find_available_ports(1).await.expect("failed to find ports");
    let port = ports[0];

    let node = TestNode::create_with_port(0, port, vec![], genesis.clone())
        .await
        .expect("failed to create node");

    // Mine some blocks
    for _ in 0..5 {
        node.mine_and_submit_block().await.expect("mining failed");
    }

    let height_before = node.height().await;
    let tip_before = node.tip_hash().await;
    assert_eq!(height_before, 5, "should have mined 5 blocks");

    // Shutdown and get restart info
    let restart_info = node.shutdown_for_restart().await;
    assert_eq!(restart_info.final_height, 5);
    assert_eq!(restart_info.final_tip, tip_before);

    // Find a new port (old one might still be in TIME_WAIT)
    let new_ports = find_available_ports(1).await.expect("failed to find ports");
    let new_port = new_ports[0];

    // Restart the node
    let restarted = restart_info
        .restart(new_port, vec![], genesis)
        .await
        .expect("failed to restart node");

    // Verify state was recovered
    let height_after = restarted.height().await;
    let tip_after = restarted.tip_hash().await;

    assert_eq!(
        height_after, height_before,
        "height should be preserved after restart"
    );
    assert_eq!(
        tip_after, tip_before,
        "tip hash should be preserved after restart"
    );

    restarted.shutdown().await;
}

/// Test that a restarted node can continue mining.
#[tokio::test]
async fn test_restarted_node_can_mine() {
    let (pk, _) = ml_dsa_87::keygen();
    let miner_address = Address::from_public_key(&pk);
    let genesis = create_test_genesis(miner_address);

    let ports = find_available_ports(1).await.expect("failed to find ports");
    let port = ports[0];

    let node = TestNode::create_with_port(0, port, vec![], genesis.clone())
        .await
        .expect("failed to create node");

    // Mine 3 blocks
    for _ in 0..3 {
        node.mine_and_submit_block().await.expect("mining failed");
    }
    assert_eq!(node.height().await, 3);

    // Shutdown and restart
    let restart_info = node.shutdown_for_restart().await;

    let new_ports = find_available_ports(1).await.expect("failed to find ports");
    let restarted = restart_info
        .restart(new_ports[0], vec![], genesis)
        .await
        .expect("failed to restart");

    assert_eq!(restarted.height().await, 3, "should start at height 3");

    // Mine 2 more blocks after restart
    for _ in 0..2 {
        restarted
            .mine_and_submit_block()
            .await
            .expect("mining failed");
    }

    assert_eq!(
        restarted.height().await,
        5,
        "should be at height 5 after mining"
    );

    restarted.shutdown().await;
}

/// Test that a restarted node rejoins the network and syncs missed blocks.
#[tokio::test]
async fn test_restarted_node_syncs_missed_blocks() {
    // Create a 2-node network
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create network");

    sleep(Duration::from_millis(500)).await;

    // Mine some blocks
    for i in 0..3 {
        network
            .node(0)
            .mine_and_submit_block()
            .await
            .expect("mining failed");
        wait_for_height(&network, i + 1, DEFAULT_TIMEOUT).await;
    }

    assert!(
        network.all_at_height(3).await,
        "both nodes should be at height 3"
    );

    // Get addresses for reconnection
    let node0_addr = network.node(0).addr();
    let genesis = network.genesis.clone();

    // Shutdown node 1 for restart
    // We need to extract node 1 from the network
    let mut nodes = network.nodes;
    let node1 = nodes.remove(1);
    let node0 = nodes.remove(0);

    let restart_info = node1.shutdown_for_restart().await;
    assert_eq!(restart_info.final_height, 3);

    // Wait for node0 to detect the disconnection before mining more blocks
    sleep(Duration::from_millis(200)).await;

    // Mine more blocks on node 0 while node 1 is offline
    for _ in 0..2 {
        node0.mine_and_submit_block().await.expect("mining failed");
    }
    assert_eq!(node0.height().await, 5, "node 0 should be at height 5");

    // Restart node 1 without seed peers first to verify persistence
    let new_ports = find_available_ports(1).await.expect("failed to find ports");
    let restarted = restart_info
        .restart(new_ports[0], vec![], genesis.clone())
        .await
        .expect("failed to restart");

    // Verify persistence - height should be 3 from before shutdown
    // (no seed peers, so no sync should have happened)
    let initial_height = restarted.height().await;
    assert_eq!(
        initial_height, 3,
        "should start at pre-shutdown height {initial_height}"
    );

    // Now shutdown and restart with connection to node0 for sync
    let restart_info2 = restarted.shutdown_for_restart().await;

    let new_ports2 = find_available_ports(1).await.expect("failed to find ports");
    let restarted = restart_info2
        .restart(new_ports2[0], vec![node0_addr], genesis)
        .await
        .expect("failed to restart with seed peer");

    // Wait for connection and sync - immediate sync is triggered on peer connect
    sleep(Duration::from_millis(2000)).await;

    // Should have synced to height 5
    let final_height = restarted.height().await;
    assert_eq!(
        final_height, 5,
        "should have synced to height 5, got {final_height}"
    );

    // Verify same tip
    assert_eq!(
        restarted.tip_hash().await,
        node0.tip_hash().await,
        "tips should match after sync"
    );

    node0.shutdown().await;
    restarted.shutdown().await;
}

/// Test multiple restart cycles.
#[tokio::test]
async fn test_multiple_restarts() {
    let (pk, _) = ml_dsa_87::keygen();
    let miner_address = Address::from_public_key(&pk);
    let genesis = create_test_genesis(miner_address);

    let ports = find_available_ports(1).await.expect("failed to find ports");

    let node = TestNode::create_with_port(0, ports[0], vec![], genesis.clone())
        .await
        .expect("failed to create node");

    // First cycle: mine 2 blocks
    node.mine_and_submit_block().await;
    node.mine_and_submit_block().await;
    assert_eq!(node.height().await, 2);

    let restart_info = node.shutdown_for_restart().await;

    // Second cycle: restart and mine 2 more
    let new_ports = find_available_ports(1).await.expect("failed to find ports");
    let node = restart_info
        .restart(new_ports[0], vec![], genesis.clone())
        .await
        .expect("failed to restart");

    assert_eq!(node.height().await, 2);
    node.mine_and_submit_block().await;
    node.mine_and_submit_block().await;
    assert_eq!(node.height().await, 4);

    let restart_info = node.shutdown_for_restart().await;

    // Third cycle: restart and verify
    let new_ports = find_available_ports(1).await.expect("failed to find ports");
    let node = restart_info
        .restart(new_ports[0], vec![], genesis)
        .await
        .expect("failed to restart");

    assert_eq!(
        node.height().await,
        4,
        "height should persist through multiple restarts"
    );

    node.shutdown().await;
}

/// Test that UTXO state is preserved across restart.
#[tokio::test]
async fn test_utxo_state_preserved() {
    let (pk, _) = ml_dsa_87::keygen();
    let miner_address = Address::from_public_key(&pk);
    let genesis = create_test_genesis(miner_address);

    let ports = find_available_ports(1).await.expect("failed to find ports");

    let node = TestNode::create_with_port(0, ports[0], vec![], genesis.clone())
        .await
        .expect("failed to create node");

    // Mine enough blocks to have mature UTXOs
    for _ in 0..102 {
        node.mine_and_submit_block().await.expect("mining failed");
    }

    // Verify we have a mature UTXO and count how many
    let utxo_before = node.find_mature_utxo().await;
    assert!(
        utxo_before.is_some(),
        "should have mature UTXO before restart"
    );

    // Get the height and tip to verify chain state
    let height_before = node.height().await;
    let tip_before = node.tip_hash().await;

    let restart_info = node.shutdown_for_restart().await;

    // Restart
    let new_ports = find_available_ports(1).await.expect("failed to find ports");
    let node = restart_info
        .restart(new_ports[0], vec![], genesis)
        .await
        .expect("failed to restart");

    // Verify chain state is preserved
    assert_eq!(
        node.height().await,
        height_before,
        "height should be preserved"
    );
    assert_eq!(node.tip_hash().await, tip_before, "tip should be preserved");

    // Verify UTXO set is preserved (we still have mature UTXOs)
    let utxo_after = node.find_mature_utxo().await;
    assert!(
        utxo_after.is_some(),
        "should still have mature UTXO after restart"
    );

    // Verify the UTXO has the same value (block reward)
    let (_, value_before) = utxo_before.unwrap();
    let (_, value_after) = utxo_after.unwrap();
    assert_eq!(value_before, value_after, "UTXO value should be the same");

    // Verify we can still create transactions (UTXO set is functional)
    let (recipient_pk, _) = ml_dsa_87::keygen();
    let recipient = Address::from_public_key(&recipient_pk);
    let tx = node.create_transaction(recipient, 1_000_000).await;
    assert!(
        tx.is_some(),
        "should be able to create transaction after restart"
    );

    node.shutdown().await;
}
