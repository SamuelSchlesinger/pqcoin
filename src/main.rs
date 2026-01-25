//! pqcoin node - A post-quantum cryptocurrency node.
//!
//! This binary runs a pqcoin node that can connect to the P2P network,
//! sync the blockchain, mine blocks, and relay transactions and blocks.

use clap::Parser;
use pqcoin::api;
use pqcoin::blockchain::{create_genesis_block, Address, Blockchain};
use pqcoin::config::Config;
use pqcoin::constants::{
    DEFAULT_DIFFICULTY, HALVING_INTERVAL, INITIAL_REWARD, TEST_DIFFICULTY_INTERVAL,
    TEST_TARGET_BLOCK_TIME,
};
use pqcoin::crypto::ml_dsa_87;
use pqcoin::miner::{mine_block, BackgroundMiner, MineResult};
use pqcoin::network::{NetworkConfig, NetworkEvent, NetworkService};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::Level;
use tracing_subscriber::EnvFilter;

/// pqcoin - Post-quantum cryptocurrency node
#[derive(Parser, Debug)]
#[command(name = "pqcoin")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Data directory for blockchain storage
    #[arg(short = 'd', long = "datadir", value_name = "DIR")]
    datadir: Option<PathBuf>,

    /// Port to listen on for P2P connections
    #[arg(short, long, value_name = "PORT")]
    port: Option<u16>,

    /// Connect to peer at startup (can be repeated)
    #[arg(short = 'C', long = "connect", value_name = "ADDR")]
    connect: Vec<SocketAddr>,

    /// Enable mining
    #[arg(short, long)]
    mine: bool,

    /// Log format: text or json
    #[arg(long, value_name = "FORMAT")]
    log_format: Option<String>,

    /// Log level: trace, debug, info, warn, error
    #[arg(long, value_name = "LEVEL")]
    log_level: Option<String>,

    /// Enable RPC/API server
    #[arg(long)]
    rpc: bool,

    /// RPC port
    #[arg(long, value_name = "PORT")]
    rpc_port: Option<u16>,

    /// RPC bind address
    #[arg(long, value_name = "ADDR")]
    rpc_bind: Option<String>,

    /// Print sample configuration and exit
    #[arg(long)]
    sample_config: bool,
}

/// Initialize logging with the given configuration.
fn init_logging(config: &pqcoin::config::LoggingConfig) {
    let filter = EnvFilter::from_default_env().add_directive(
        config
            .level
            .parse()
            .unwrap_or_else(|_| Level::INFO.into()),
    );

    if config.format == "json" {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .json()
            .init();
    } else {
        tracing_subscriber::fmt().with_env_filter(filter).init();
    }
}

#[tokio::main]
async fn main() {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Handle --sample-config
    if cli.sample_config {
        println!("{}", Config::sample());
        return;
    }

    // Load configuration
    let mut config = match Config::load(cli.config.as_ref()) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Error loading configuration: {}", e);
            std::process::exit(1);
        }
    };

    // Apply CLI overrides
    config.apply_cli_overrides(
        cli.port,
        &cli.connect,
        cli.mine,
        cli.log_level.as_deref(),
        cli.log_format.as_deref(),
        cli.rpc,
        cli.rpc_port,
        cli.rpc_bind.as_deref(),
    );
    config.apply_datadir_override(cli.datadir.as_ref());

    // Initialize logging
    init_logging(&config.logging);

    // Load miner keypair from wallet if available, otherwise generate temporary
    let wallet_path = pqcoin::wallet::Wallet::default_path();
    let (_miner_keypair, miner_address) = if config.mining.enabled && wallet_path.exists() {
        match pqcoin::wallet::WalletFile::load(&wallet_path) {
            Ok(wallet_file) => {
                // Try password from PQCOIN_WALLET_PASSWORD env var
                let password = std::env::var("PQCOIN_WALLET_PASSWORD").ok();

                let keypair = if let Some(pw) = password {
                    wallet_file.decrypt(&pw).ok()
                } else {
                    // Prompt for password
                    eprint!("Enter wallet password for mining: ");
                    let mut pw = String::new();
                    if std::io::stdin().read_line(&mut pw).is_ok() {
                        wallet_file.decrypt(pw.trim()).ok()
                    } else {
                        None
                    }
                };

                if let Some(kp) = keypair {
                    let addr = kp.address();
                    tracing::info!(
                        address = %addr,
                        "loaded miner keypair from wallet"
                    );
                    (Some(kp), addr)
                } else {
                    tracing::warn!("couldn't decrypt wallet, generating temporary miner keypair");
                    let (pk, _sk) = ml_dsa_87::keygen();
                    let addr = Address::from_public_key(&pk);
                    (None, addr)
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to load wallet, generating temporary miner keypair");
                let (pk, _sk) = ml_dsa_87::keygen();
                let addr = Address::from_public_key(&pk);
                (None, addr)
            }
        }
    } else {
        let (pk, _sk) = ml_dsa_87::keygen();
        let addr = Address::from_public_key(&pk);
        tracing::info!(
            address = %addr,
            "generated temporary miner address (no wallet found)"
        );
        (None, addr)
    };

    // Create the genesis block (all nodes must use the same genesis)
    // Use a fixed address for genesis so all nodes have the same chain
    let genesis_address = Address::from_hash(pqcoin::crypto::hash(b"pqcoin genesis"));
    let genesis = create_genesis_block(0, DEFAULT_DIFFICULTY, INITIAL_REWARD, genesis_address);

    // Initialize the blockchain with persistent storage
    // Use test constants for faster block times during development
    tracing::info!(
        storage_path = %config.storage.path.display(),
        "opening blockchain storage"
    );
    let blockchain = match Blockchain::open(
        &config.storage.path,
        genesis,
        TEST_DIFFICULTY_INTERVAL,
        TEST_TARGET_BLOCK_TIME,
        INITIAL_REWARD,
        HALVING_INTERVAL,
    ) {
        Ok(chain) => {
            tracing::info!(
                height = chain.height(),
                tip = %chain.tip_hash().to_hex()[..16],
                "blockchain loaded from storage"
            );
            chain
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to open blockchain storage");
            std::process::exit(1);
        }
    };
    let blockchain = Arc::new(RwLock::new(blockchain));

    // Configure the network
    let listen_addr: SocketAddr = format!("0.0.0.0:{}", config.network.port).parse().unwrap();
    let num_seed_peers = config.network.seed_peers.len();
    let network_config = NetworkConfig {
        listen_addr,
        max_peers: config.network.max_peers,
        max_outbound: config.network.max_outbound,
        seed_peers: config.network.seed_peers.clone(),
    };

    tracing::info!(
        port = config.network.port,
        seed_peers = num_seed_peers,
        mine = config.mining.enabled,
        rpc = config.rpc.enabled,
        "starting pqcoin node"
    );

    // Create the network service
    let mut service = NetworkService::new(blockchain.clone(), network_config);
    let mempool = service.mempool();
    let block_submitter = service.block_submitter();

    // Take the event receiver
    let mut events = service
        .take_event_receiver()
        .expect("event receiver already taken");

    // Create miner control
    let background_miner = Arc::new(BackgroundMiner::new());

    // Spawn event handler task
    let blockchain_events = blockchain.clone();
    let miner_for_events = background_miner.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                NetworkEvent::PeerConnected { peer_id, addr } => {
                    tracing::info!(peer_id = peer_id, addr = %addr, "peer connected");
                }
                NetworkEvent::PeerDisconnected { peer_id, addr } => {
                    tracing::info!(peer_id = peer_id, addr = %addr, "peer disconnected");
                }
                NetworkEvent::NewBlock(block) => {
                    let height = blockchain_events.read().await.height();
                    tracing::info!(
                        hash = %block.hash().to_hex()[..16],
                        height = height,
                        txs = block.transactions.len(),
                        "received new block"
                    );
                    // Stop current mining when new block received
                    miner_for_events.stop();
                }
                NetworkEvent::NewTransaction(tx) => {
                    tracing::debug!(
                        txid = %tx.txid().to_hex()[..16],
                        inputs = tx.inputs.len(),
                        outputs = tx.outputs.len(),
                        "received new transaction"
                    );
                }
                NetworkEvent::SyncStateChanged(state) => {
                    tracing::info!(state = ?state, "sync state changed");
                }
            }
        }
    });

    // Start API server if enabled
    if config.rpc.enabled {
        let api_state = api::ApiState::new(blockchain.clone(), mempool.clone(), &service);
        let rpc_addr = format!("{}:{}", config.rpc.bind, config.rpc.port);
        let metrics_addr = format!("{}:{}", config.rpc.bind, config.rpc.metrics_port);

        tracing::info!(
            rpc_addr = %rpc_addr,
            metrics_addr = %metrics_addr,
            "starting API servers"
        );

        // Spawn RPC server
        let api_state_rpc = api_state.clone();
        tokio::spawn(async move {
            if let Err(e) = api::run_rpc_server(api_state_rpc, &rpc_addr).await {
                tracing::error!(error = %e, "RPC server error");
            }
        });

        // Spawn metrics/health server
        tokio::spawn(async move {
            if let Err(e) = api::run_metrics_server(api_state, &metrics_addr).await {
                tracing::error!(error = %e, "metrics server error");
            }
        });
    }

    // Spawn mining task if enabled
    if config.mining.enabled {
        let blockchain_miner = blockchain.clone();
        let mempool_miner = mempool.clone();
        let miner_ctl = background_miner.clone();
        let block_tx = block_submitter;

        tokio::spawn(async move {
            tracing::info!("starting miner");

            loop {
                // Reset stop flag for new mining round
                miner_ctl.reset();
                let stop_flag = miner_ctl.stop_flag();

                // Get blockchain and mempool snapshots
                let chain_snapshot = blockchain_miner.read().await;
                let mempool_snapshot = mempool_miner.read().await;

                tracing::info!(
                    height = chain_snapshot.height(),
                    mempool_txs = mempool_snapshot.len(),
                    "starting mining round"
                );

                // Clone what we need before spawning blocking task
                let chain_clone = (*chain_snapshot).clone();
                let mempool_clone = (*mempool_snapshot).clone();
                drop(chain_snapshot);
                drop(mempool_snapshot);

                // Mine in a blocking task to not block the async runtime
                let result = tokio::task::spawn_blocking(move || {
                    mine_block(&chain_clone, &mempool_clone, miner_address, stop_flag)
                })
                .await;

                match result {
                    Ok(MineResult::Success(block)) => {
                        let hash = block.hash();
                        let height = blockchain_miner.read().await.height() + 1;

                        // Add block to blockchain
                        let added_to_main_chain = {
                            let mut chain = blockchain_miner.write().await;
                            match chain.add_block(block.clone()) {
                                Ok(true) => true,   // Extended main chain
                                Ok(false) => false, // Side chain or already known
                                Err(e) => {
                                    tracing::warn!(error = %e, "failed to add mined block");
                                    continue;
                                }
                            }
                        };

                        // Only update mempool if our block was added to the main chain
                        // If another block won the race, the network handler will update mempool
                        if added_to_main_chain {
                            let mut mp = mempool_miner.write().await;
                            mp.remove_confirmed(&block.transactions);

                            tracing::info!(
                                hash = %hash.to_hex()[..16],
                                height = height,
                                txs = block.transactions.len(),
                                "mined new block!"
                            );

                            // Send block to be broadcast to network
                            let _ = block_tx.send(block).await;
                        } else {
                            tracing::debug!(
                                hash = %hash.to_hex()[..16],
                                "mined block rejected - another block won the race"
                            );
                        }
                    }
                    Ok(MineResult::Stopped) => {
                        tracing::debug!("mining stopped for new block");
                    }
                    Ok(MineResult::NoWork) => {
                        tracing::debug!("no mining work available");
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "mining task panicked");
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }

    // Run the network service
    if let Err(e) = service.run().await {
        tracing::error!(error = %e, "network service error");
        std::process::exit(1);
    }
}
