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
