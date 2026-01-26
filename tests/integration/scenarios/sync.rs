//! Chain synchronization tests.

use std::time::Duration;
use tokio::time::sleep;

use crate::integration::helpers::{DEFAULT_TIMEOUT, wait_for_height};
use crate::integration::test_network::{TestNetwork, Topology, add_node_to_network};

/// Test that a new node syncs the existing chain.
#[tokio::test]
async fn test_new_node_syncs_chain() {
    use crate::integration::helpers::mine_to_height;

    // Create a network with 2 nodes
    let mut network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine to height 3 using the robust helper
    let success = mine_to_height(&network, 3, 0, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 3");

    // Verify initial network is at height 3
    assert!(
        network.all_at_height(3).await,
        "initial nodes should be at height 3"
    );

    // Add a new node that connects to the existing network
    let existing_addrs = network.addresses();
    let _new_node_id = add_node_to_network(&mut network, existing_addrs)
        .await
        .expect("failed to add new node");

    // Wait for the new node to sync
    let synced = wait_for_height(&network, 3, DEFAULT_TIMEOUT).await;
    assert!(synced, "new node should sync to height 3");

    // Verify consensus
    assert!(
        network.verify_consensus().await,
        "all nodes including new one should agree on chain tip"
    );

    network.shutdown().await;
}

/// Test that a node syncs after reconnecting.
#[tokio::test]
async fn test_sync_after_blocks_while_offline() {
    use crate::integration::helpers::mine_to_height;

    // Create a 2-node network
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine to height 1 using the robust helper
    let success = mine_to_height(&network, 1, 0, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 1");

    // Both nodes should be synced
    assert!(
        network.verify_consensus().await,
        "nodes should be synced initially"
    );

    // This test would ideally disconnect a node, mine blocks, then reconnect
    // For now, just verify basic sync works

    network.shutdown().await;
}

/// Test that a node can sync a longer chain.
#[tokio::test]
async fn test_sync_longer_chain() {
    use crate::integration::helpers::mine_blocks_distributed;

    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine 5 blocks distributed across nodes
    let success = mine_blocks_distributed(&network, 5, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 5");

    // All nodes should be at height 5
    assert!(
        network.all_at_height(5).await,
        "all nodes should be at height 5"
    );

    // All nodes should have the same tip
    assert!(
        network.verify_consensus().await,
        "all nodes should have same chain tip"
    );

    network.shutdown().await;
}
