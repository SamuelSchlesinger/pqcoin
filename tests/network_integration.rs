//! Multi-node integration tests for pqcoin network.
//!
//! Run all tests: `cargo test --test network_integration`
//! Run specific scenario: `cargo test --test network_integration peer_connection`
//! Run with logging: `RUST_LOG=pqcoin=debug cargo test --test network_integration -- --nocapture`

mod integration;
