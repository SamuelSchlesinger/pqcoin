//! Checkpoint validation for preventing long-range attacks.

use super::error::BlockchainError;
use crate::crypto::Hash;

/// Hardcoded checkpoints for known-good blocks.
/// Format: (height, block_hash_hex)
/// These prevent long-range attacks during initial sync.
const CHECKPOINTS: &[(u64, &str)] = &[
    // Genesis block - add real hash after launch
    // (0, "genesis_hash_here"),
];

/// Block hash for which we assume all ancestors have valid signatures.
/// Set to None to verify all signatures (slower but more secure).
/// This should be a block that is buried under significant PoW.
#[allow(dead_code)]
pub(crate) const ASSUME_VALID: Option<&str> = None;

/// Parse a hex string into a Hash.
/// Returns None if the hex string is invalid or wrong length.
fn parse_hash_hex(hex_str: &str) -> Option<Hash> {
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.len() != 64 {
        return None;
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Some(Hash::from_bytes(arr))
}

/// Validate a block hash against hardcoded checkpoints.
///
/// Returns Ok(()) if the block passes checkpoint validation, or an error
/// if the block hash doesn't match the expected checkpoint at this height.
pub(crate) fn validate_checkpoint(height: u64, block_hash: Hash) -> Result<(), BlockchainError> {
    for &(checkpoint_height, checkpoint_hash_hex) in CHECKPOINTS {
        if height == checkpoint_height {
            if let Some(expected_hash) = parse_hash_hex(checkpoint_hash_hex) {
                if block_hash != expected_hash {
                    return Err(BlockchainError::CheckpointMismatch {
                        height,
                        expected: expected_hash,
                        got: block_hash,
                    });
                }
            }
        }
    }
    Ok(())
}
