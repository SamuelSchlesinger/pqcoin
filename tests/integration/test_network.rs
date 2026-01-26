//! TestNetwork - A collection of test nodes with configurable topology.

use pqcoin::blockchain::{Address, Block};
use pqcoin::crypto::{Hash, PublicKey, SecretKey, ml_dsa_87};
use std::net::SocketAddr;
use tokio::net::TcpListener;

use super::helpers::create_test_genesis;
use super::test_node::TestNode;

/// Network topology for connecting test nodes.
#[derive(Debug, Clone, Copy)]
pub enum Topology {
    /// Every node connects to every other node.
    FullMesh,
    /// Nodes form a chain: 0 -> 1 -> 2 -> ...
    Linear,
    /// All nodes connect to node 0 (hub and spoke).
    Star,
    /// No initial connections.
    None,
}

/// A test network managing multiple nodes.
pub struct TestNetwork {
    /// The test nodes.
    pub nodes: Vec<TestNode>,
    /// Keypairs for all nodes (for signing transactions).
    keypairs: Vec<(PublicKey, SecretKey)>,
    /// Common miner address (recipient of genesis block reward).
    pub miner_address: Address,
    /// Genesis block used by all nodes.
    pub genesis: Block,
}

impl TestNetwork {
    /// Create a new test network with the specified number of nodes and topology.
    pub async fn new(
        node_count: usize,
        topology: Topology,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        assert!(node_count > 0, "node count must be at least 1");

        // Generate a keypair for the genesis block miner
        let (genesis_pk, genesis_sk) = ml_dsa_87::keygen();
        let miner_address = Address::from_public_key(&genesis_pk);

        // Create shared genesis block
        let genesis = create_test_genesis(miner_address);

        // Find available ports for all nodes
        let ports = find_available_ports(node_count).await?;

        // Build seed peer lists based on topology
        let seed_peers_list = build_seed_peers(&ports, topology);

        // Create all nodes
        let mut nodes = Vec::with_capacity(node_count);
        let mut keypairs = vec![(genesis_pk, genesis_sk)];

        for (id, (port, seed_peers)) in ports.iter().zip(seed_peers_list.iter()).enumerate() {
            let node =
                TestNode::create_with_port(id, *port, seed_peers.clone(), genesis.clone()).await?;
            keypairs.push(node.keypair.clone());
            nodes.push(node);
        }

        Ok(Self {
            nodes,
            keypairs,
            miner_address,
            genesis,
        })
    }

    /// Get the number of nodes in the network.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Get a reference to a node by index.
    pub fn node(&self, index: usize) -> &TestNode {
        &self.nodes[index]
    }

    /// Get a mutable reference to a node by index.
    pub fn node_mut(&mut self, index: usize) -> &mut TestNode {
        &mut self.nodes[index]
    }

    /// Check if all nodes have at least one peer connection.
    pub async fn all_connected(&self) -> bool {
        if self.nodes.len() <= 1 {
            return true; // Single node is trivially "connected"
        }

        for node in &self.nodes {
            if node.peer_count().await == 0 {
                return false;
            }
        }
        true
    }

    /// Check if all nodes are at the specified height.
    pub async fn all_at_height(&self, target_height: u64) -> bool {
        for node in &self.nodes {
            if node.height().await != target_height {
                return false;
            }
        }
        true
    }

    /// Check if all nodes have a specific block.
    pub async fn all_have_block(&self, block_hash: Hash) -> bool {
        for node in &self.nodes {
            if !node.has_block(block_hash).await {
                return false;
            }
        }
        true
    }

    /// Check if all nodes have a specific transaction in their mempool.
    pub async fn all_have_tx_in_mempool(&self, txid: Hash) -> bool {
        for node in &self.nodes {
            if !node.has_tx_in_mempool(txid).await {
                return false;
            }
        }
        true
    }

    /// Check if all nodes have the same chain tip.
    pub async fn verify_consensus(&self) -> bool {
        if self.nodes.is_empty() {
            return true;
        }

        let first_tip = self.nodes[0].tip_hash().await;
        for node in self.nodes.iter().skip(1) {
            if node.tip_hash().await != first_tip {
                return false;
            }
        }
        true
    }

    /// Get all node addresses.
    pub fn addresses(&self) -> Vec<SocketAddr> {
        self.nodes.iter().map(|n| n.addr()).collect()
    }

    /// Shutdown all nodes gracefully.
    pub async fn shutdown(self) {
        for node in self.nodes {
            node.shutdown().await;
        }
    }
}

/// Find N available ports by binding to port 0 and extracting the assigned port.
pub async fn find_available_ports(count: usize) -> Result<Vec<u16>, std::io::Error> {
    let mut ports = Vec::with_capacity(count);

    for _ in 0..count {
        // Bind to port 0 to get an ephemeral port
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        ports.push(port);
        // Listener is dropped here, freeing the port
        // There's a small race condition window, but it's acceptable for tests
    }

    Ok(ports)
}

/// Build seed peer lists based on topology.
#[allow(clippy::needless_range_loop)]
fn build_seed_peers(ports: &[u16], topology: Topology) -> Vec<Vec<SocketAddr>> {
    let n = ports.len();
    let mut seed_peers = vec![Vec::new(); n];

    match topology {
        Topology::FullMesh => {
            // Each node connects to all previous nodes
            for i in 1..n {
                for j in 0..i {
                    let addr: SocketAddr = format!("127.0.0.1:{}", ports[j]).parse().unwrap();
                    seed_peers[i].push(addr);
                }
            }
        }
        Topology::Linear => {
            // Each node connects to the previous node
            for i in 1..n {
                let addr: SocketAddr = format!("127.0.0.1:{}", ports[i - 1]).parse().unwrap();
                seed_peers[i].push(addr);
            }
        }
        Topology::Star => {
            // All nodes connect to node 0
            for i in 1..n {
                let addr: SocketAddr = format!("127.0.0.1:{}", ports[0]).parse().unwrap();
                seed_peers[i].push(addr);
            }
        }
        Topology::None => {
            // No initial connections
        }
    }

    seed_peers
}

/// Add a new node to an existing network.
pub async fn add_node_to_network(
    network: &mut TestNetwork,
    seed_peers: Vec<SocketAddr>,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let id = network.nodes.len();
    let ports = find_available_ports(1).await?;
    let port = ports[0];

    let node = TestNode::create_with_port(id, port, seed_peers, network.genesis.clone()).await?;

    network.keypairs.push(node.keypair.clone());
    network.nodes.push(node);

    Ok(id)
}
