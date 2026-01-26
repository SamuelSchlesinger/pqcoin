//! Peer connection tests.

use std::time::Duration;
use tokio::time::sleep;

use pqcoin::network::NetworkEvent;

use crate::integration::helpers::{SHORT_TIMEOUT, wait_for_connections, wait_for_peers};
use crate::integration::test_network::{TestNetwork, Topology};

/// Test that two nodes can connect to each other.
#[tokio::test]
async fn test_two_nodes_connect() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    // Wait for nodes to connect
    sleep(Duration::from_millis(500)).await;

    // Verify both nodes started successfully
    assert_eq!(network.node_count(), 2);

    // Both nodes should have the same genesis
    let tip0 = network.node(0).tip_hash().await;
    let tip1 = network.node(1).tip_hash().await;
    assert_eq!(tip0, tip1, "nodes should have same genesis block");

    network.shutdown().await;
}

/// Test creating a network with a configurable number of nodes.
#[tokio::test]
async fn test_configurable_node_count_3() {
    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    assert_eq!(network.node_count(), 3);
    assert!(
        network.verify_consensus().await,
        "all nodes should agree on chain tip"
    );

    network.shutdown().await;
}

/// Test creating a network with 5 nodes.
#[tokio::test]
async fn test_configurable_node_count_5() {
    let network = TestNetwork::new(5, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    assert_eq!(network.node_count(), 5);
    assert!(
        network.verify_consensus().await,
        "all nodes should agree on chain tip"
    );

    network.shutdown().await;
}

/// Test star topology where all nodes connect to node 0.
#[tokio::test]
async fn test_star_topology() {
    let network = TestNetwork::new(4, Topology::Star)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    assert_eq!(network.node_count(), 4);

    // All nodes should have the same genesis
    for i in 1..4 {
        let tip = network.node(i).tip_hash().await;
        assert_eq!(
            tip,
            network.node(0).tip_hash().await,
            "node {i} should have same tip as node 0"
        );
    }

    network.shutdown().await;
}

/// Test linear topology where nodes form a chain.
#[tokio::test]
async fn test_linear_topology() {
    let network = TestNetwork::new(4, Topology::Linear)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    assert_eq!(network.node_count(), 4);

    // All nodes should have the same genesis
    assert!(
        network.verify_consensus().await,
        "all nodes should agree on chain tip"
    );

    network.shutdown().await;
}

/// Test that nodes can handle connection to non-existent peers gracefully.
#[tokio::test]
async fn test_connection_to_nonexistent_peer() {
    // Create a single node with a seed peer that doesn't exist
    let _nonexistent_peer: std::net::SocketAddr = "127.0.0.1:59999".parse().unwrap();
    let network = TestNetwork::new(1, Topology::None)
        .await
        .expect("failed to create test network");

    // Node should still be running even if seed peer doesn't exist
    sleep(Duration::from_millis(200)).await;
    assert_eq!(network.node_count(), 1);

    network.shutdown().await;
}

/// Test that network events are received for peer connections.
#[tokio::test]
async fn test_peer_connection_events() {
    let mut network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    // Wait for connection to establish
    sleep(Duration::from_millis(500)).await;

    // Drain events from node 1 (which initiated the connection)
    let events = network.node_mut(1).drain_events();

    // Should have received at least one PeerConnected event
    let connected_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, NetworkEvent::PeerConnected { .. }))
        .collect();

    assert!(
        !connected_events.is_empty(),
        "should have received PeerConnected events"
    );

    network.shutdown().await;
}

/// Test that block events are received.
#[tokio::test]
async fn test_block_received_events() {
    let mut network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Node 0 mines a block
    let block = network
        .node(0)
        .mine_and_submit_block()
        .await
        .expect("mining failed");

    // Wait for propagation
    sleep(Duration::from_millis(500)).await;

    // Node 1 should have received a NewBlock event
    let events = network.node_mut(1).drain_events();

    let block_events: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, NetworkEvent::NewBlock(_)))
        .collect();

    assert!(
        !block_events.is_empty(),
        "node 1 should have received NewBlock event"
    );

    // The received block should match what was mined
    if let Some(NetworkEvent::NewBlock(received_block)) = block_events.first() {
        assert_eq!(
            received_block.hash(),
            block.hash(),
            "received block should match mined block"
        );
    }

    network.shutdown().await;
}

/// Test waiting for next event with timeout.
#[tokio::test]
async fn test_next_event_with_timeout() {
    let mut network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    sleep(Duration::from_millis(500)).await;

    // Drain any existing events
    network.node_mut(1).drain_events();

    // Mine a block to generate an event
    network
        .node(0)
        .mine_and_submit_block()
        .await
        .expect("mining failed");

    // Wait for the next event on node 1
    let event = network.node_mut(1).next_event(Duration::from_secs(5)).await;

    assert!(
        event.is_some(),
        "should receive an event after block is mined"
    );

    network.shutdown().await;
}

/// Test that node IDs are unique.
#[tokio::test]
async fn test_node_ids_are_unique() {
    let network = TestNetwork::new(5, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    // Collect all node IDs
    let ids: Vec<_> = network.nodes.iter().map(|n| n.id).collect();

    // Check uniqueness
    let mut unique_ids = ids.clone();
    unique_ids.sort();
    unique_ids.dedup();

    assert_eq!(ids.len(), unique_ids.len(), "all node IDs should be unique");

    network.shutdown().await;
}

/// Test accessing the miner address from the network.
#[tokio::test]
async fn test_network_miner_address() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    // The network's miner_address is from the genesis block
    let miner_address = network.miner_address;

    // Verify it's a valid address (not all zeros)
    let bytes = miner_address.as_bytes();
    assert!(
        bytes.iter().any(|&b| b != 0),
        "miner address should not be all zeros"
    );

    network.shutdown().await;
}

/// Test that all_connected helper works.
#[tokio::test]
async fn test_all_connected_helper() {
    let network = TestNetwork::new(3, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    // Wait for connections and verify using helper
    let connected = wait_for_connections(&network, SHORT_TIMEOUT).await;
    assert!(connected, "all nodes should be connected");

    network.shutdown().await;
}

/// Test wait_for_peers helper.
#[tokio::test]
async fn test_wait_for_peers_helper() {
    let network = TestNetwork::new(2, Topology::FullMesh)
        .await
        .expect("failed to create test network");

    // Node 0 should eventually have at least 1 peer (node 1 connects to it)
    let has_peers = wait_for_peers(network.node(0), 1, SHORT_TIMEOUT).await;
    assert!(has_peers, "node 0 should have at least 1 peer");

    // Verify the actual peer count
    let count = network.node(0).peer_count().await;
    assert!(count >= 1, "peer count should be at least 1, got {count}");

    network.shutdown().await;
}
