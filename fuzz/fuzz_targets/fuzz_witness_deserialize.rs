//! Fuzz target for Witness deserialization.
//!
//! Tests that arbitrary byte sequences cannot crash the witness deserializer.
//! The deserializer should return an error for invalid input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pqcoin::blockchain::{Deserialize, Witness};

fuzz_target!(|data: &[u8]| {
    // Attempt to deserialize arbitrary bytes as a Witness.
    // This should never panic, only return Ok or Err.
    let _ = Witness::deserialize(data);
});
