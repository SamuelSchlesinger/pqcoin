//! BlockHeader type containing block metadata and proof-of-work.

use super::serialize::{
    Deserialize, DeserializeError, Serialize, read_fixed_bytes, read_u32, read_u64, write_u32,
    write_u64,
};
use crate::constants::DIFFICULTY_COEFFICIENT_MASK;
use crate::crypto::{self, Hash};

/// The header of a block, containing metadata and proof-of-work.
///
/// The block header is the portion of the block that is hashed for
/// proof-of-work. It contains all the information needed to validate
/// the block's place in the chain without the full transaction data.
///
/// # Proof of Work
///
/// The proof-of-work requires finding a nonce such that the SHA3-512 hash
/// of the header is less than or equal to the target. The target is derived
/// from the difficulty bits using the formula:
///
/// ```text
/// target = coefficient * 2^(8 * (exponent - 3))
/// ```
///
/// where `difficulty_bits = (exponent << 24) | coefficient`.
///
/// # Serialization Format
///
/// | Field           | Size     | Description                           |
/// |-----------------|----------|---------------------------------------|
/// | version         | 4 bytes  | Block version (little-endian)         |
/// | prev_hash       | 64 bytes | Hash of the previous block header     |
/// | merkle_root     | 64 bytes | Merkle root of transactions           |
/// | timestamp       | 8 bytes  | Unix timestamp (little-endian)        |
/// | difficulty_bits | 4 bytes  | Compact difficulty target             |
/// | nonce           | 8 bytes  | Proof-of-work nonce (little-endian)   |
///
/// Total: 152 bytes
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BlockHeader {
    /// Block format version.
    pub version: u32,
    /// Hash of the previous block header.
    pub prev_hash: Hash,
    /// Merkle root of the transactions in this block.
    pub merkle_root: Hash,
    /// Unix timestamp when the block was mined.
    pub timestamp: u64,
    /// Compact representation of the difficulty target.
    pub difficulty_bits: u32,
    /// Nonce used to achieve the required proof-of-work (32 bytes).
    ///
    /// A 32-byte nonce provides 2^256 possible values, making nonce exhaustion
    /// impossible and eliminating the need for extraNonce mechanisms.
    pub nonce: [u8; 32],
}

impl BlockHeader {
    /// The current block version.
    pub const CURRENT_VERSION: u32 = 1;

    /// Header size in bytes (4 + 64 + 64 + 8 + 4 + 32 = 176).
    pub const SIZE: usize = 176;

    /// Compute the hash of this block header.
    pub fn hash(&self) -> Hash {
        crypto::hash(&self.to_bytes())
    }

    /// Decode difficulty bits into a 512-bit target.
    ///
    /// The target is a 64-byte big-endian integer. A valid proof-of-work
    /// requires the block hash to be less than or equal to this target.
    pub fn target(&self) -> [u8; 64] {
        let exponent = (self.difficulty_bits >> 24) as usize;
        let coefficient = self.difficulty_bits & DIFFICULTY_COEFFICIENT_MASK;

        let mut target = [0u8; 64];

        if exponent == 0 || exponent > 64 {
            return target;
        }

        // The target formula is: target = coefficient * 2^(8 * (exponent - 3))
        // The coefficient is a 3-byte big-endian number placed at position (64 - exponent)

        if exponent >= 3 {
            // Normal case: coefficient fits in target array
            let coef_bytes = [
                ((coefficient >> 16) & 0xFF) as u8,
                ((coefficient >> 8) & 0xFF) as u8,
                (coefficient & 0xFF) as u8,
            ];

            let start = 64usize.saturating_sub(exponent);
            for (i, &byte) in coef_bytes.iter().enumerate() {
                if start + i < 64 {
                    target[start + i] = byte;
                }
            }
        } else {
            // Very easy target - coefficient shifted right
            // This case is rare (exponent 1 or 2) and represents extremely easy difficulty
            // Shift coefficient right by (3 - exponent) bytes
            let shift_bytes = 3 - exponent;
            let shifted_coef = coefficient >> (8 * shift_bytes);

            // Place the remaining coefficient bytes at the end of the array
            if shift_bytes == 2 {
                // exponent = 1: only 1 byte of coefficient remains
                target[63] = (shifted_coef & 0xFF) as u8;
            } else if shift_bytes == 1 {
                // exponent = 2: 2 bytes of coefficient remain
                target[62] = ((shifted_coef >> 8) & 0xFF) as u8;
                target[63] = (shifted_coef & 0xFF) as u8;
            }
        }

        target
    }

    /// Check if the block hash satisfies the proof-of-work requirement.
    pub fn check_pow(&self) -> bool {
        let hash = self.hash();
        let target = self.target();

        // Compare hash to target (both as big-endian 512-bit integers)
        // Hash must be <= target
        for (h, t) in hash.as_bytes().iter().zip(target.iter()) {
            if h < t {
                return true;
            }
            if h > t {
                return false;
            }
        }
        true // Equal
    }

    /// Encode a target into difficulty bits.
    ///
    /// This is the inverse of `target()`.
    pub fn encode_target(target: &[u8; 64]) -> u32 {
        // Find the first non-zero byte
        let mut first_nonzero = 0;
        for (i, &byte) in target.iter().enumerate() {
            if byte != 0 {
                first_nonzero = i;
                break;
            }
        }

        // Extract 3 coefficient bytes starting at first_nonzero
        let mut coefficient = 0u32;
        for i in 0..3 {
            if first_nonzero + i < 64 {
                coefficient = (coefficient << 8) | (target[first_nonzero + i] as u32);
            }
        }

        // Exponent is 64 - first_nonzero
        let exponent = (64 - first_nonzero) as u32;

        // If the high bit of coefficient is set, we need to adjust
        // to avoid it being interpreted as negative
        if coefficient & 0x00800000 != 0 {
            coefficient >>= 8;
            ((exponent + 1) << 24) | coefficient
        } else {
            (exponent << 24) | coefficient
        }
    }
}

impl std::fmt::Debug for BlockHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockHeader")
            .field("version", &self.version)
            .field(
                "prev_hash",
                &format!("{}...", &self.prev_hash.to_hex()[..16]),
            )
            .field(
                "merkle_root",
                &format!("{}...", &self.merkle_root.to_hex()[..16]),
            )
            .field("timestamp", &self.timestamp)
            .field(
                "difficulty_bits",
                &format!("0x{:08x}", self.difficulty_bits),
            )
            .field("nonce", &self.nonce)
            .finish()
    }
}

impl Serialize for BlockHeader {
    fn serialize(&self, buf: &mut Vec<u8>) {
        write_u32(buf, self.version);
        buf.extend_from_slice(self.prev_hash.as_bytes());
        buf.extend_from_slice(self.merkle_root.as_bytes());
        write_u64(buf, self.timestamp);
        write_u32(buf, self.difficulty_bits);
        buf.extend_from_slice(&self.nonce);
    }
}

impl Deserialize for BlockHeader {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (version, data) = read_u32(data)?;
        let (prev_hash, data) = read_fixed_bytes::<64>(data)?;
        let (merkle_root, data) = read_fixed_bytes::<64>(data)?;
        let (timestamp, data) = read_u64(data)?;
        let (difficulty_bits, data) = read_u32(data)?;
        let (nonce, data) = read_fixed_bytes::<32>(data)?;
        Ok((
            BlockHeader {
                version,
                prev_hash: Hash::from_bytes(prev_hash),
                merkle_root: Hash::from_bytes(merkle_root),
                timestamp,
                difficulty_bits,
                nonce,
            },
            data,
        ))
    }
}
