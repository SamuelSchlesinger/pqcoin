//! Fuzz target for variable-length integer (varint) parsing.
//!
//! Tests that arbitrary byte sequences cannot crash the varint reader.
//! The reader should return an error for invalid input, never panic.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pqcoin::blockchain::read_var_int;

fuzz_target!(|data: &[u8]| {
    // Attempt to read a varint from arbitrary bytes.
    // This should never panic, only return Ok or Err.
    let _ = read_var_int(data);
});
