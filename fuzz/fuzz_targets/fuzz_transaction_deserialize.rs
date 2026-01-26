//! Fuzz target for Transaction deserialization.
//!
//! Tests that arbitrary byte sequences cannot crash the transaction deserializer.
//! The deserializer should return an error for invalid input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pqcoin::blockchain::{Deserialize, Transaction};

fuzz_target!(|data: &[u8]| {
    // Attempt to deserialize arbitrary bytes as a Transaction.
    // This should never panic, only return Ok or Err.
    let _ = Transaction::deserialize(data);
});
