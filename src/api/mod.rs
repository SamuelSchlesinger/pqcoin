//! HTTP API for pqcoin.
//!
//! This module provides:
//! - JSON-RPC API for wallet and node interaction
//! - Prometheus metrics endpoint
//! - Health check endpoint

mod health;
mod metrics;
mod rpc;

use crate::blockchain::Blockchain;
use crate::mempool::Mempool;
use crate::network::NetworkService;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

pub use health::health_router;
pub use metrics::MetricsRegistry;
pub use rpc::{run_rpc_server, PqcoinRpcServer};

/// Shared state for all API endpoints.
#[derive(Clone)]
pub struct ApiState {
    /// Reference to the blockchain.
    pub blockchain: Arc<RwLock<Blockchain>>,
    /// Reference to the mempool.
    pub mempool: Arc<RwLock<Mempool>>,
    /// Metrics registry.
    pub metrics: Arc<MetricsRegistry>,
    /// Connected peer count (updated externally).
    pub peer_count: Arc<AtomicU64>,
    /// Total blocks mined counter.
    pub blocks_mined: Arc<AtomicU64>,
}

impl ApiState {
    /// Create a new API state.
    pub fn new(
        blockchain: Arc<RwLock<Blockchain>>,
        mempool: Arc<RwLock<Mempool>>,
        network: &NetworkService,
    ) -> Self {
        let _ = network; // Used for peer count in future
        Self {
            blockchain,
            mempool,
            metrics: Arc::new(MetricsRegistry::new()),
            peer_count: Arc::new(AtomicU64::new(0)),
            blocks_mined: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Update the peer count.
    pub fn set_peer_count(&self, count: u64) {
        self.peer_count.store(count, Ordering::Relaxed);
    }

    /// Get the current peer count.
    pub fn get_peer_count(&self) -> u64 {
        self.peer_count.load(Ordering::Relaxed)
    }

    /// Increment the blocks mined counter.
    pub fn increment_blocks_mined(&self) {
        self.blocks_mined.fetch_add(1, Ordering::Relaxed);
    }

    /// Get the total blocks mined count.
    pub fn get_blocks_mined(&self) -> u64 {
        self.blocks_mined.load(Ordering::Relaxed)
    }
}

/// Run the metrics/health HTTP server.
pub async fn run_metrics_server(state: ApiState, addr: &str) -> Result<(), std::io::Error> {
    use axum::Router;
    use tokio::net::TcpListener;

    let app = Router::new()
        .merge(health::health_router())
        .merge(metrics::metrics_router())
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    tracing::info!(addr = addr, "metrics/health server listening");

    axum::serve(listener, app).await?;
    Ok(())
}
