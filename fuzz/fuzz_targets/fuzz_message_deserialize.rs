//! Fuzz target for network Message deserialization.
//!
//! Tests that arbitrary byte sequences cannot crash the message deserializer.
//! The deserializer should return an error for invalid input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pqcoin::network::Message;

fuzz_target!(|data: &[u8]| {
    // Attempt to deserialize arbitrary bytes as a network Message.
    // This should never panic, only return Ok or Err.
    let _ = Message::deserialize_with_header(data);
});
