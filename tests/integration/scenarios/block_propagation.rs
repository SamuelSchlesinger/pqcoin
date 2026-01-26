//! Block propagation tests.

use std::time::Duration;
use tokio::time::sleep;

use crate::integration::helpers::{
    DEFAULT_TIMEOUT, SHORT_TIMEOUT, mine_to_height, wait_for_block_propagation,
};
use crate::integration::test_network::{TestNetwork, Topology};

/// Test that a block mined by one node propagates to all nodes.
#[tokio::test]
async fn test_block_propagates_to_all_nodes() {
    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    // Wait for connections to establish
    sleep(Duration::from_millis(500)).await;

    // Initial state check
    assert!(
        network.all_at_height(0).await,
        "all nodes should start at height 0"
    );

    // Mine to height 1 using robust helper
    let success = mine_to_height(&network, 1, 0, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 1");

    // Get the block hash from node 0's tip
    let block_hash = network.node(0).tip_hash().await;

    // Verify all nodes have the same tip
    assert!(
        network.verify_consensus().await,
        "all nodes should agree on chain tip after block propagation"
    );

    // Verify all nodes have the block using all_have_block
    assert!(
        network.all_have_block(block_hash).await,
        "all nodes should have the mined block"
    );

    network.shutdown().await;
}

/// Test that blocks propagate through a linear network.
#[tokio::test]
async fn test_block_propagates_through_linear_network() {
    let network = TestNetwork::new(4, Topology::Linear)
        .await
        .expect("failed to create test network");

    // Wait for connections
    sleep(Duration::from_millis(500)).await;

    // Mine to height 1 from node 0 (start of chain)
    let success = mine_to_height(&network, 1, 0, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 1");

    let block_hash = network.node(0).tip_hash().await;

    // Block should propagate through the chain: 0 -> 1 -> 2 -> 3
    let propagated = wait_for_block_propagation(&network, block_hash, DEFAULT_TIMEOUT).await;
    assert!(
        propagated,
        "block should propagate through linear network within timeout"
    );

    // All nodes should have the same tip
    assert!(
        network.verify_consensus().await,
        "all nodes should reach consensus"
    );

    network.shutdown().await;
}

/// Test that multiple blocks propagate correctly.
#[tokio::test]
async fn test_multiple_blocks_propagate() {
    use crate::integration::helpers::mine_blocks_distributed;

    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine 3 blocks distributed across nodes
    let success = mine_blocks_distributed(&network, 3, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 3");

    // Final check
    assert!(
        network.all_at_height(3).await,
        "all nodes should be at height 3"
    );
    assert!(
        network.verify_consensus().await,
        "all nodes should agree on chain tip"
    );

    network.shutdown().await;
}

/// Test block propagation with different nodes mining.
#[tokio::test]
async fn test_different_miners() {
    use crate::integration::helpers::mine_blocks_distributed;

    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine 3 blocks distributed across nodes
    let success = mine_blocks_distributed(&network, 3, DEFAULT_TIMEOUT).await;
    assert!(success, "failed to mine to height 3");

    assert!(
        network.all_at_height(3).await,
        "all nodes should be at height 3"
    );

    assert!(
        network.verify_consensus().await,
        "all nodes should agree on chain tip"
    );

    network.shutdown().await;
}

/// Test block propagation with quick timeout.
#[tokio::test]
async fn test_block_propagates_quickly() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine to height 1 using robust helper with short timeout
    let success = mine_to_height(&network, 1, 0, SHORT_TIMEOUT).await;
    assert!(success, "failed to mine to height 1");

    let block_hash = network.node(0).tip_hash().await;

    // Block should already be propagated (mine_to_height waits for it)
    let propagated = wait_for_block_propagation(&network, block_hash, SHORT_TIMEOUT).await;
    assert!(
        propagated,
        "block should propagate quickly in a 2-node network"
    );

    network.shutdown().await;
}

/// Test that mining without submission doesn't propagate.
#[tokio::test]
async fn test_mine_without_submit() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine a block but don't submit it
    let block = network.node(0).mine_block().await.expect("mining failed");
    let block_hash = block.hash();

    // Wait a bit
    sleep(Duration::from_millis(200)).await;

    // Node 1 should NOT have the block (it wasn't submitted)
    assert!(
        !network.node(1).has_block(block_hash).await,
        "unmitted block should not propagate"
    );

    // Now submit it
    network
        .node(0)
        .submit_block(block.clone())
        .await
        .expect("submit failed");

    // Add to local blockchain manually for this test
    {
        let mut blockchain = network.node(0).blockchain.write().await;
        let _ = blockchain.add_block(block);
    }

    // Now it should propagate
    let propagated = wait_for_block_propagation(&network, block_hash, DEFAULT_TIMEOUT).await;
    assert!(propagated, "submitted block should propagate");

    network.shutdown().await;
}
