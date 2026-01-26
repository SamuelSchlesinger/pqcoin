//! Prometheus metrics for pqcoin.

use axum::Router;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use prometheus_client::encoding::text::encode;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use std::sync::atomic::AtomicI64;

use super::ApiState;

/// Prometheus metrics registry for pqcoin.
pub struct MetricsRegistry {
    registry: Registry,
    /// Current blockchain height.
    pub blockchain_height: Gauge<i64, AtomicI64>,
    /// Number of connected peers.
    pub connected_peers: Gauge<i64, AtomicI64>,
    /// Number of transactions in the mempool.
    pub mempool_size: Gauge<i64, AtomicI64>,
    /// Total blocks mined by this node.
    pub blocks_mined_total: Counter,
    /// Total transactions received.
    pub transactions_received_total: Counter,
    /// Total blocks received from network.
    pub blocks_received_total: Counter,
}

impl MetricsRegistry {
    /// Create a new metrics registry with all pqcoin metrics.
    pub fn new() -> Self {
        let mut registry = Registry::default();

        let blockchain_height = Gauge::<i64, AtomicI64>::default();
        registry.register(
            "pqcoin_blockchain_height",
            "Current blockchain height",
            blockchain_height.clone(),
        );

        let connected_peers = Gauge::<i64, AtomicI64>::default();
        registry.register(
            "pqcoin_connected_peers",
            "Number of connected peers",
            connected_peers.clone(),
        );

        let mempool_size = Gauge::<i64, AtomicI64>::default();
        registry.register(
            "pqcoin_mempool_size",
            "Number of transactions in the mempool",
            mempool_size.clone(),
        );

        let blocks_mined_total = Counter::default();
        registry.register(
            "pqcoin_blocks_mined_total",
            "Total number of blocks mined by this node",
            blocks_mined_total.clone(),
        );

        let transactions_received_total = Counter::default();
        registry.register(
            "pqcoin_transactions_received_total",
            "Total number of transactions received",
            transactions_received_total.clone(),
        );

        let blocks_received_total = Counter::default();
        registry.register(
            "pqcoin_blocks_received_total",
            "Total number of blocks received from network",
            blocks_received_total.clone(),
        );

        Self {
            registry,
            blockchain_height,
            connected_peers,
            mempool_size,
            blocks_mined_total,
            transactions_received_total,
            blocks_received_total,
        }
    }

    /// Encode all metrics in Prometheus text format.
    pub fn encode(&self) -> String {
        let mut buffer = String::new();
        encode(&mut buffer, &self.registry).expect("encoding should succeed");
        buffer
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// GET /metrics - Prometheus metrics endpoint.
async fn metrics_handler(State(state): State<ApiState>) -> impl IntoResponse {
    // Update gauges with current values
    let blockchain = state.blockchain.read().await;
    let mempool = state.mempool.read().await;
    let peer_count = state.get_peer_count().await;

    state
        .metrics
        .blockchain_height
        .set(blockchain.height() as i64);
    state.metrics.connected_peers.set(peer_count as i64);
    state.metrics.mempool_size.set(mempool.len() as i64);

    drop(blockchain);
    drop(mempool);

    // Encode and return
    let output = state.metrics.encode();
    (
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        output,
    )
}

/// Create the metrics router.
pub fn metrics_router() -> Router<ApiState> {
    Router::new().route("/metrics", get(metrics_handler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_registry_creation() {
        let registry = MetricsRegistry::new();
        registry.blockchain_height.set(100);
        registry.connected_peers.set(10);
        registry.mempool_size.set(50);
        registry.blocks_mined_total.inc();

        let output = registry.encode();
        assert!(output.contains("pqcoin_blockchain_height"));
        assert!(output.contains("pqcoin_connected_peers"));
        assert!(output.contains("pqcoin_mempool_size"));
        assert!(output.contains("pqcoin_blocks_mined_total"));
    }
}
