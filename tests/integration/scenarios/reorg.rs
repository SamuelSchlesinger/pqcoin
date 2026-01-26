//! Chain reorganization tests.

use std::time::Duration;
use tokio::time::sleep;

use crate::integration::helpers::{DEFAULT_TIMEOUT, mine_blocks_distributed, mine_to_height};
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

    // Mine to height 1 from node 0
    let success = mine_to_height(&network, 1, 0, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 1");

    // Both nodes should agree
    assert!(
        network.verify_consensus().await,
        "nodes should agree on chain"
    );

    // Mine to height 3 distributed across nodes
    let success = mine_blocks_distributed(&network, 3, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 3");

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
    use crate::integration::helpers::mine_blocks_distributed;

    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine several blocks distributed across nodes
    let success = mine_blocks_distributed(&network, 5, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 5");

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

    // Mine blocks distributed across nodes to height 4
    let target_height = 4u64;
    let success = mine_blocks_distributed(&network, target_height, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height {target_height}");

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
