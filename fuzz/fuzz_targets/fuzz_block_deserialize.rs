//! Fuzz target for Block deserialization.
//!
//! Tests that arbitrary byte sequences cannot crash the block deserializer.
//! The deserializer should return an error for invalid input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pqcoin::blockchain::{Block, Deserialize};

fuzz_target!(|data: &[u8]| {
    // Attempt to deserialize arbitrary bytes as a Block.
    // This should never panic, only return Ok or Err.
    let _ = Block::deserialize(data);
});
