//! Integration test framework for multi-node pqcoin network testing.
//!
//! This module provides abstractions for:
//! - `TestNode`: A single in-process pqcoin node
//! - `TestNetwork`: A collection of nodes with configurable topology
//! - Helper utilities for waiting on network events
//! - `ChaosNetwork`: Failure injection and resilience testing

pub mod chaos;
pub mod helpers;
pub mod scenarios;
pub mod test_network;
pub mod test_node;
