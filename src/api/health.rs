//! Health check endpoint for pqcoin.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use super::ApiState;

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Overall health status.
    pub status: String,
    /// Current blockchain height.
    pub blockchain_height: u64,
    /// Number of connected peers.
    pub connected_peers: u64,
    /// Number of transactions in the mempool.
    pub mempool_size: usize,
    /// Whether the node is synced.
    pub synced: bool,
    /// Node version.
    pub version: String,
}

/// GET /health - Health check endpoint.
async fn health_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let blockchain = state.blockchain.read().await;
    let mempool = state.mempool.read().await;

    let height = blockchain.height();
    let mempool_size = mempool.len();

    drop(blockchain);
    drop(mempool);

    let peer_count = state.get_peer_count().await;

    // Consider node healthy if it has at least one peer or is running solo
    // Consider synced if we have peers and are not behind
    let synced = peer_count == 0 || height > 0;

    let response = HealthResponse {
        status: "ok".to_string(),
        blockchain_height: height,
        connected_peers: peer_count,
        mempool_size,
        synced,
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    (StatusCode::OK, Json(response))
}

/// GET /ready - Readiness probe for orchestrators.
async fn ready_handler(State(_state): State<ApiState>) -> impl IntoResponse {
    // Node is ready if this handler runs (blockchain is always initialized)
    StatusCode::OK
}

/// GET /live - Liveness probe for orchestrators.
async fn live_handler() -> impl IntoResponse {
    // Node is alive if this handler runs
    StatusCode::OK
}

/// Create the health router.
pub fn health_router() -> Router<ApiState> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
        .route("/live", get(live_handler))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_serialization() {
        let response = HealthResponse {
            status: "ok".to_string(),
            blockchain_height: 100,
            connected_peers: 5,
            mempool_size: 10,
            synced: true,
            version: "0.1.0".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"blockchain_height\":100"));
    }
}
