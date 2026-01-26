//! Transaction propagation tests.

use std::time::Duration;
use tokio::time::sleep;

use pqcoin::COINBASE_MATURITY;
use pqcoin::blockchain::Address;
use pqcoin::crypto::ml_dsa_87;

use crate::integration::helpers::{DEFAULT_TIMEOUT, wait_for_height, wait_for_tx_in_mempools};
use crate::integration::test_network::{TestNetwork, Topology};

/// Mine enough blocks for node 0 to have a mature UTXO.
///
/// We need COINBASE_MATURITY + 1 blocks so that the coinbase from block 1
/// (the first block node 0 mines) is mature.
async fn mine_to_maturity(network: &TestNetwork) {
    let blocks_needed = COINBASE_MATURITY + 1;
    for i in 0..blocks_needed {
        network
            .node(0)
            .mine_and_submit_block()
            .await
            .expect("mining failed");

        // Wait for propagation every 10 blocks
        if i % 10 == 9 || i == blocks_needed - 1 {
            wait_for_height(network, i + 1, DEFAULT_TIMEOUT).await;
        }
    }
}

/// Test that a transaction propagates to all nodes' mempools.
#[tokio::test]
async fn test_tx_propagates_to_all_nodes() {
    let mut network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine enough blocks so node 0 has a mature UTXO
    mine_to_maturity(&network).await;

    // Create a transaction from node 0 to a new address
    let (recipient_pk, _) = ml_dsa_87::keygen();
    let recipient_address = Address::from_public_key(&recipient_pk);

    let tx = network
        .node(0)
        .create_transaction(recipient_address, 1_000_000)
        .await
        .expect("failed to create transaction");

    let txid = tx.txid();

    // Add the transaction to node 0's mempool
    network
        .node_mut(0)
        .add_to_mempool(tx)
        .await
        .expect("failed to add tx to mempool");

    // Verify node 0 has it
    assert!(
        network.node(0).has_tx_in_mempool(txid).await,
        "node 0 should have the transaction in mempool"
    );

    // Verify mempool has exactly 1 transaction
    assert_eq!(
        network.node(0).mempool_size().await,
        1,
        "node 0 mempool should have 1 transaction"
    );

    network.shutdown().await;
}

/// Test that a transaction is included in a mined block.
#[tokio::test]
async fn test_tx_included_in_mined_block() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine enough blocks for maturity
    mine_to_maturity(&network).await;

    let height_before = network.node(0).height().await;

    // Create a transaction
    let (recipient_pk, _) = ml_dsa_87::keygen();
    let recipient_address = Address::from_public_key(&recipient_pk);

    let tx = network
        .node(0)
        .create_transaction(recipient_address, 1_000_000)
        .await
        .expect("failed to create transaction");

    let txid = tx.txid();

    // Add to mempool
    network
        .node(0)
        .add_to_mempool(tx)
        .await
        .expect("failed to add tx to mempool");

    // Mine a block - it should include the transaction
    let block = network
        .node(0)
        .mine_and_submit_block()
        .await
        .expect("mining failed");

    // The transaction should be in the block (position 1, after coinbase)
    assert!(
        block.transactions.len() > 1,
        "block should contain more than just coinbase"
    );

    let block_txids: Vec<_> = block.transactions.iter().map(|tx| tx.txid()).collect();
    assert!(
        block_txids.contains(&txid),
        "mined block should contain the transaction"
    );

    // Transaction should no longer be in mempool
    assert!(
        !network.node(0).has_tx_in_mempool(txid).await,
        "transaction should be removed from mempool after mining"
    );

    // Wait for block to propagate
    wait_for_height(&network, height_before + 1, DEFAULT_TIMEOUT).await;

    // All nodes should have the block
    assert!(
        network.verify_consensus().await,
        "all nodes should agree on chain tip"
    );

    network.shutdown().await;
}

/// Test that all nodes can verify they have a transaction in mempool.
#[tokio::test]
async fn test_mempool_contains_check() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine blocks for maturity
    mine_to_maturity(&network).await;

    // Create transaction
    let (recipient_pk, _) = ml_dsa_87::keygen();
    let recipient_address = Address::from_public_key(&recipient_pk);

    let tx = network
        .node(0)
        .create_transaction(recipient_address, 500_000)
        .await
        .expect("failed to create transaction");

    let txid = tx.txid();

    // Initially no nodes should have this transaction
    assert!(
        !network.all_have_tx_in_mempool(txid).await,
        "no node should have the tx initially"
    );

    // Add to node 0
    network
        .node(0)
        .add_to_mempool(tx)
        .await
        .expect("failed to add tx");

    // Now node 0 has it, but not all nodes
    assert!(
        network.node(0).has_tx_in_mempool(txid).await,
        "node 0 should have the tx"
    );

    network.shutdown().await;
}

/// Test creating multiple transactions.
#[tokio::test]
async fn test_multiple_transactions() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Mine blocks to mature multiple coinbases
    // We need COINBASE_MATURITY + extra blocks to have multiple spendable UTXOs
    let blocks_needed = COINBASE_MATURITY + 5;
    for i in 0..blocks_needed {
        network
            .node(0)
            .mine_and_submit_block()
            .await
            .expect("mining failed");

        if i % 10 == 9 || i == blocks_needed - 1 {
            wait_for_height(&network, i + 1, DEFAULT_TIMEOUT).await;
        }
    }

    // Create a recipient
    let (recipient_pk, _) = ml_dsa_87::keygen();
    let recipient_address = Address::from_public_key(&recipient_pk);

    // Create first transaction
    let tx1 = network
        .node(0)
        .create_transaction(recipient_address, 100_000)
        .await
        .expect("failed to create first transaction");

    network
        .node(0)
        .add_to_mempool(tx1)
        .await
        .expect("failed to add first tx");

    assert_eq!(network.node(0).mempool_size().await, 1);

    // Mine a block to confirm the first transaction
    network
        .node(0)
        .mine_and_submit_block()
        .await
        .expect("mining failed");

    // Mempool should be empty now
    assert_eq!(
        network.node(0).mempool_size().await,
        0,
        "mempool should be empty after mining"
    );

    // Create second transaction (from a different UTXO)
    let tx2 = network
        .node(0)
        .create_transaction(recipient_address, 200_000)
        .await
        .expect("failed to create second transaction");

    network
        .node(0)
        .add_to_mempool(tx2)
        .await
        .expect("failed to add second tx");

    assert_eq!(network.node(0).mempool_size().await, 1);

    network.shutdown().await;
}

/// Test using wait_for_tx_in_mempools helper.
#[tokio::test]
async fn test_wait_for_tx_helper() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    mine_to_maturity(&network).await;

    // Create a transaction
    let (recipient_pk, _) = ml_dsa_87::keygen();
    let recipient_address = Address::from_public_key(&recipient_pk);

    let tx = network
        .node(0)
        .create_transaction(recipient_address, 100_000)
        .await
        .expect("failed to create transaction");

    let txid = tx.txid();

    // Transaction not in any mempool yet
    let found = wait_for_tx_in_mempools(&network, txid, Duration::from_millis(100)).await;
    assert!(!found, "tx should not be in mempools yet");

    // Add to node 0
    network
        .node(0)
        .add_to_mempool(tx)
        .await
        .expect("failed to add tx");

    // Still not in ALL mempools (only node 0 has it)
    let found = wait_for_tx_in_mempools(&network, txid, Duration::from_millis(100)).await;
    assert!(!found, "tx should not be in ALL mempools");

    network.shutdown().await;
}
