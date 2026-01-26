//! Fuzz target for LockingCondition deserialization.
//!
//! Tests that arbitrary byte sequences cannot crash the locking condition deserializer.
//! The deserializer should return an error for invalid input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pqcoin::blockchain::{Deserialize, LockingCondition};

fuzz_target!(|data: &[u8]| {
    // Attempt to deserialize arbitrary bytes as a LockingCondition.
    // This should never panic, only return Ok or Err.
    let _ = LockingCondition::deserialize(data);
});
