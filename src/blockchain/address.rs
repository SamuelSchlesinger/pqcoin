//! Address type representing a destination for funds.

use crate::crypto::{self, Hash, PublicKey};
use super::serialize::{Deserialize, DeserializeError, Serialize, read_fixed_bytes};

/// A pqcoin address, which is the SHA3-512 hash of a public key.
///
/// Addresses are used in P2PKH outputs to specify who can spend the funds.
/// The owner must provide a public key that hashes to this address, along
/// with a valid signature.
///
/// # Size
///
/// Addresses are 64 bytes (512 bits), the full SHA3-512 output.
///
/// # Example
///
/// ```
/// use pqcoin::crypto::ml_dsa_87;
/// use pqcoin::blockchain::Address;
///
/// let (public_key, _) = ml_dsa_87::keygen();
/// let address = Address::from_public_key(&public_key);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Address(Hash);

impl Address {
    /// Create an address from a public key by hashing it.
    pub fn from_public_key(pk: &PublicKey) -> Self {
        Self(crypto::hash(pk.as_ref()))
    }

    /// Create an address from raw hash bytes.
    pub fn from_hash(hash: Hash) -> Self {
        Self(hash)
    }

    /// Get the underlying hash.
    pub fn as_hash(&self) -> &Hash {
        &self.0
    }

    /// Get the address as bytes.
    pub fn as_bytes(&self) -> &[u8; 64] {
        self.0.as_bytes()
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Address({})", &self.to_hex()[..16])
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Display first 16 hex chars for readability
        write!(f, "{}...", &self.to_hex()[..16])
    }
}

impl Serialize for Address {
    fn serialize(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.0.as_bytes());
    }
}

impl Deserialize for Address {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (bytes, data) = read_fixed_bytes::<64>(data)?;
        Ok((Address(Hash::from_bytes(bytes)), data))
    }
}
