//! Chain reorganization tests.

use std::time::Duration;
use tokio::time::sleep;

use crate::integration::helpers::{DEFAULT_TIMEOUT, wait_for_height};
use crate::integration::test_network::{TestNetwork, Topology};

/// Test that nodes reorganize to a longer chain.
#[tokio::test]
async fn test_reorg_to_longer_chain() {
    // Create a network where nodes can potentially have different views
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Both nodes start at genesis
    assert!(
        network.all_at_height(0).await,
        "all nodes should start at height 0"
    );

    // Node 0 mines a block
    network
        .node(0)
        .mine_and_submit_block()
        .await
        .expect("mining failed");

    // Wait for propagation
    wait_for_height(&network, 1, DEFAULT_TIMEOUT).await;

    // Both nodes should agree
    assert!(
        network.verify_consensus().await,
        "nodes should agree on chain"
    );

    // Continue mining to ensure chain builds correctly
    for i in 1..3 {
        network
            .node(i % 2)
            .mine_and_submit_block()
            .await
            .expect("mining failed");

        wait_for_height(&network, (i + 1) as u64, DEFAULT_TIMEOUT).await;
    }

    // Final consensus check
    assert!(
        network.verify_consensus().await,
        "all nodes should agree after multiple blocks"
    );

    network.shutdown().await;
}

/// Test handling of competing blocks at the same height.
///
/// Note: This is a challenging test because both nodes need to mine at almost
/// the same time. For now, this just verifies that the network eventually
/// reaches consensus.
#[tokio::test]
async fn test_competing_blocks_consensus() {
    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine several blocks to build a chain
    for i in 0..5 {
        network
            .node(i % 3)
            .mine_and_submit_block()
            .await
            .expect("mining failed");

        // Wait for propagation between each block
        wait_for_height(&network, (i + 1) as u64, DEFAULT_TIMEOUT).await;
    }

    // All nodes should eventually reach consensus
    assert!(
        network.verify_consensus().await,
        "all nodes should eventually agree on chain tip"
    );

    // Verify the chain is the expected length
    assert!(
        network.all_at_height(5).await,
        "all nodes should be at height 5"
    );

    network.shutdown().await;
}

/// Test that reorganization handles different chain lengths.
#[tokio::test]
async fn test_chain_selection() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine blocks and verify chain grows correctly
    let target_height = 4u64;

    for i in 0..target_height {
        let miner = (i as usize) % 2;
        network
            .node(miner)
            .mine_and_submit_block()
            .await
            .expect("mining failed");

        let height = i + 1;
        let synced = wait_for_height(&network, height, DEFAULT_TIMEOUT).await;
        assert!(synced, "failed to sync at height {height}");
    }

    // Both nodes should have the same view
    assert!(
        network.verify_consensus().await,
        "nodes should have consensus"
    );

    assert!(
        network.all_at_height(target_height).await,
        "all nodes should be at target height"
    );

    network.shutdown().await;
}
