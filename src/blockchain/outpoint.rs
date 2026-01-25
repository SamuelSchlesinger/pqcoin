//! OutPoint type representing a reference to a transaction output.

use super::serialize::{
    Deserialize, DeserializeError, Serialize, read_fixed_bytes, read_u32, write_u32,
};
use crate::crypto::Hash;

/// A reference to a specific output of a previous transaction.
///
/// An outpoint uniquely identifies a transaction output by combining the
/// transaction ID (hash) with the index of the output within that transaction.
///
/// # Serialization Format
///
/// | Field  | Size     | Description                    |
/// |--------|----------|--------------------------------|
/// | txid   | 64 bytes | SHA3-512 hash of the transaction |
/// | index  | 4 bytes  | Output index (little-endian u32) |
///
/// Total: 68 bytes
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutPoint {
    /// The transaction ID (hash of the transaction).
    pub txid: Hash,
    /// The index of the output within the transaction.
    pub index: u32,
}

impl OutPoint {
    /// Create a new outpoint.
    pub fn new(txid: Hash, index: u32) -> Self {
        Self { txid, index }
    }

    /// Create a null outpoint (used for coinbase transactions).
    pub fn null() -> Self {
        Self {
            txid: Hash::from_bytes([0u8; 64]),
            index: u32::MAX,
        }
    }

    /// Check if this is a null outpoint.
    pub fn is_null(&self) -> bool {
        self.index == u32::MAX && self.txid.as_bytes().iter().all(|&b| b == 0)
    }
}

impl std::fmt::Debug for OutPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OutPoint({}:{}, {})",
            &self.txid.to_hex()[..8],
            &self.txid.to_hex()[120..],
            self.index
        )
    }
}

impl Serialize for OutPoint {
    fn serialize(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.txid.as_bytes());
        write_u32(buf, self.index);
    }
}

impl Deserialize for OutPoint {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (txid_bytes, data) = read_fixed_bytes::<64>(data)?;
        let (index, data) = read_u32(data)?;
        Ok((
            OutPoint {
                txid: Hash::from_bytes(txid_bytes),
                index,
            },
            data,
        ))
    }
}
