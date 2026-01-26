//! Utility functions for integration tests.

use pqcoin::blockchain::{Address, Block, create_genesis_block};
use pqcoin::crypto::Hash;
use std::future::Future;
use std::time::Duration;
use tokio::time::{sleep, timeout};

use super::test_network::TestNetwork;
use super::test_node::TestNode;

/// Easy difficulty for instant mining in tests (0x40ffffff).
pub const TEST_DIFFICULTY: u32 = 0x40ffffff;

/// Default timeout for test operations.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Long timeout for high-block-count operations (like mining to maturity).
pub const LONG_TIMEOUT: Duration = Duration::from_secs(60);

/// Short timeout for quick operations.
pub const SHORT_TIMEOUT: Duration = Duration::from_secs(5);

/// Poll interval for wait functions.
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Create a test genesis block with easy difficulty.
pub fn create_test_genesis(miner_address: Address) -> Block {
    create_genesis_block(0, TEST_DIFFICULTY, 50_000_000, miner_address)
}

/// Wait for a condition to become true within a timeout.
///
/// Polls the condition function at regular intervals until it returns `true`
/// or the timeout expires.
pub async fn wait_for<F, Fut>(
    condition: F,
    timeout_duration: Duration,
    poll_interval: Duration,
) -> bool
where
    F: Fn() -> Fut,
    Fut: Future<Output = bool>,
{
    let result = timeout(timeout_duration, async {
        loop {
            if condition().await {
                return true;
            }
            sleep(poll_interval).await;
        }
    })
    .await;

    result.unwrap_or(false)
}

/// Wait for a node to have at least the specified number of peers.
pub async fn wait_for_peers(node: &TestNode, min_peers: usize, timeout_duration: Duration) -> bool {
    wait_for(
        || async { node.peer_count().await >= min_peers },
        timeout_duration,
        POLL_INTERVAL,
    )
    .await
}

/// Wait for all nodes in the network to be connected according to topology.
pub async fn wait_for_connections(network: &TestNetwork, timeout_duration: Duration) -> bool {
    wait_for(
        || async { network.all_connected().await },
        timeout_duration,
        POLL_INTERVAL,
    )
    .await
}

/// Wait for all nodes to reach a specific blockchain height.
pub async fn wait_for_height(
    network: &TestNetwork,
    target_height: u64,
    timeout_duration: Duration,
) -> bool {
    wait_for(
        || async { network.all_at_height(target_height).await },
        timeout_duration,
        POLL_INTERVAL,
    )
    .await
}

/// Wait for a block to propagate to all nodes in the network.
pub async fn wait_for_block_propagation(
    network: &TestNetwork,
    block_hash: Hash,
    timeout_duration: Duration,
) -> bool {
    wait_for(
        || async { network.all_have_block(block_hash).await },
        timeout_duration,
        POLL_INTERVAL,
    )
    .await
}

/// Wait for a transaction to appear in all mempools.
pub async fn wait_for_tx_in_mempools(
    network: &TestNetwork,
    txid: Hash,
    timeout_duration: Duration,
) -> bool {
    wait_for(
        || async { network.all_have_tx_in_mempool(txid).await },
        timeout_duration,
        POLL_INTERVAL,
    )
    .await
}

/// Mine blocks until the network reaches the target height.
///
/// This is a robust alternative to calling `mine_and_submit_block` directly.
/// It handles race conditions by:
/// 1. Checking if the target height is already reached
/// 2. Mining from the specified node with retries
/// 3. Waiting for propagation after each successful mine
///
/// Returns true if the target height was reached within the timeout.
pub async fn mine_to_height(
    network: &TestNetwork,
    target_height: u64,
    miner_index: usize,
    timeout_duration: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout_duration;

    loop {
        // Check if we've already reached the target
        let current_height = network.node(miner_index).height().await;
        if current_height >= target_height {
            // Wait for all nodes to sync
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            return wait_for_height(network, target_height, remaining).await;
        }

        // Check timeout
        if tokio::time::Instant::now() >= deadline {
            return false;
        }

        // Try to mine a block
        let result = network.node(miner_index).mine_and_submit_block().await;

        if result.is_some() {
            // Mining succeeded, wait a bit for propagation
            sleep(Duration::from_millis(50)).await;
        } else {
            // Mining returned None - either failed or block already added via sync
            // Check if height progressed anyway
            let new_height = network.node(miner_index).height().await;
            if new_height > current_height {
                // Progress was made (possibly by another node), continue
                sleep(Duration::from_millis(20)).await;
            } else {
                // No progress, wait a bit before retrying
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

/// Mine a sequence of blocks from alternating nodes to reach a target height.
///
/// This distributes mining across nodes to simulate realistic network behavior.
/// Returns true if the target height was reached within the timeout.
pub async fn mine_blocks_distributed(
    network: &TestNetwork,
    target_height: u64,
    timeout_duration: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout_duration;
    let node_count = network.node_count();

    let mut current_miner = 0;

    loop {
        // Check if we've already reached the target on any node
        let mut max_height = 0u64;
        for i in 0..node_count {
            let h = network.node(i).height().await;
            if h > max_height {
                max_height = h;
            }
        }

        if max_height >= target_height {
            // Wait for all nodes to sync to this height
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            return wait_for_height(network, target_height, remaining).await;
        }

        // Check timeout
        if tokio::time::Instant::now() >= deadline {
            return false;
        }

        // Try to mine a block from the current miner
        let result = network.node(current_miner).mine_and_submit_block().await;

        if result.is_some() {
            // Give network time to propagate
            let next_height = max_height + 1;
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let propagated =
                wait_for_height(network, next_height, remaining.min(Duration::from_secs(5))).await;
            if !propagated {
                // Propagation timeout - continue anyway
                sleep(Duration::from_millis(100)).await;
            }
        } else {
            // Short delay before trying the next miner
            sleep(Duration::from_millis(50)).await;
        }

        // Rotate to next miner
        current_miner = (current_miner + 1) % node_count;
    }
}
