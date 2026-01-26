//! Chaos testing framework for pqcoin.
//!
//! This module provides tools for testing network resilience under adverse conditions:
//! - Random node crashes and restarts
//! - Network partitions
//! - Continuous mining with failure injection
//! - Invariant checking and statistics collection

mod chaos_network;
mod config;
mod events;
mod invariants;
mod stats;

pub use chaos_network::ChaosNetwork;
pub use config::ChaosConfig;
