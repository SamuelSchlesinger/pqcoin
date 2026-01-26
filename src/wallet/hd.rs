//! Hierarchical Deterministic (HD) key derivation for pqcoin.
//!
//! This module provides HD wallet functionality using a BIP-44-like path structure:
//! `m/44'/pqc'/account'/change/index`
//!
//! Unlike BIP-32, which uses HMAC-SHA512, we use SHA3-512 for derivation to align
//! with pqcoin's post-quantum cryptographic primitives.

use crate::crypto::hash;
use crate::wallet::KeyPair;

use super::mnemonic::MasterSeed;

/// The coin type for pqcoin in BIP-44 path.
/// Using a placeholder value until official registration.
pub const COIN_TYPE: u32 = 0x7071_6300; // "pqc\0" in ASCII

/// HD wallet derivation context.
#[derive(Clone)]
pub struct HdDeriver {
    /// The master seed.
    master_seed: MasterSeed,
    /// The current derivation index.
    index: u32,
}

impl HdDeriver {
    /// Create a new HD deriver from a master seed.
    pub fn new(master_seed: MasterSeed) -> Self {
        Self {
            master_seed,
            index: 0,
        }
    }

    /// Get the current derivation index.
    pub fn current_index(&self) -> u32 {
        self.index
    }

    /// Set the derivation index.
    pub fn set_index(&mut self, index: u32) {
        self.index = index;
    }

    /// Derive a keypair at the given path.
    ///
    /// Path format: `m/44'/pqc'/0'/0/index`
    /// - 44' = BIP-44 purpose
    /// - pqc' = pqcoin coin type
    /// - 0' = account 0
    /// - 0 = external chain
    /// - index = address index
    pub fn derive(&self, index: u32) -> KeyPair {
        let path = format!("m/44'/{COIN_TYPE}''/0'/0/{index}");
        self.derive_path(&path)
    }

    /// Derive a keypair using a custom path string.
    ///
    /// The path is hashed with the master seed to produce the key material.
    pub fn derive_path(&self, path: &str) -> KeyPair {
        let mut input = Vec::with_capacity(64 + path.len());
        input.extend_from_slice(self.master_seed.as_bytes());
        input.extend_from_slice(path.as_bytes());

        // Hash to get 64 bytes of key material
        let key_material = hash(&input);

        // Generate a deterministic keypair from the key material
        // ML-DSA-87 requires specific seed format, so we use the hash as entropy
        generate_keypair_from_entropy(key_material.as_bytes())
    }

    /// Derive the next keypair and increment the index.
    pub fn derive_next(&mut self) -> KeyPair {
        let keypair = self.derive(self.index);
        self.index = self.index.saturating_add(1);
        keypair
    }
}

impl std::fmt::Debug for HdDeriver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HdDeriver")
            .field("master_seed", &"[REDACTED]")
            .field("index", &self.index)
            .finish()
    }
}

/// Generate a keypair from entropy.
///
/// This function creates a deterministic ML-DSA-87 keypair from the given entropy.
/// The entropy is expanded using SHA3-512 to provide enough random data for key generation.
fn generate_keypair_from_entropy(entropy: &[u8]) -> KeyPair {
    use crate::crypto::ml_dsa_87;

    // ML-DSA-87 keygen needs internal randomness, but we want determinism.
    // We'll use the entropy to seed an expansion function that provides
    // enough pseudo-random bytes for key generation.

    // Expand entropy to sufficient size for key generation
    // ML-DSA-87 needs about 32 bytes of seed for deterministic keygen
    let mut expanded = Vec::with_capacity(128);
    let mut current = hash(entropy);
    expanded.extend_from_slice(current.as_bytes());

    for i in 0..3 {
        let mut round_input = Vec::with_capacity(65);
        round_input.extend_from_slice(current.as_bytes());
        round_input.push(i);
        current = hash(&round_input);
        expanded.extend_from_slice(current.as_bytes());
    }

    // Use the first 32 bytes as the seed
    let mut seed = [0u8; 32];
    seed.copy_from_slice(&expanded[..32]);

    // Generate the keypair using pqcrypto's deterministic API if available,
    // otherwise we fall back to non-deterministic generation seeded by entropy
    let (public_key, secret_key) = ml_dsa_87::keypair_from_seed(&seed);

    KeyPair::from_keys(public_key, secret_key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::mnemonic::Mnemonic;

    #[test]
    fn test_derive_deterministic() {
        let mnemonic = Mnemonic::generate();
        let seed = mnemonic.to_seed("");

        let deriver1 = HdDeriver::new(seed.clone());
        let deriver2 = HdDeriver::new(seed);

        let key1 = deriver1.derive(0);
        let key2 = deriver2.derive(0);

        assert_eq!(key1.public_key().to_bytes(), key2.public_key().to_bytes());
    }

    #[test]
    fn test_different_indices() {
        let mnemonic = Mnemonic::generate();
        let seed = mnemonic.to_seed("");

        let deriver = HdDeriver::new(seed);

        let key0 = deriver.derive(0);
        let key1 = deriver.derive(1);

        assert_ne!(key0.public_key().to_bytes(), key1.public_key().to_bytes());
    }

    #[test]
    fn test_derive_next() {
        let mnemonic = Mnemonic::generate();
        let seed = mnemonic.to_seed("");

        let mut deriver = HdDeriver::new(seed.clone());

        let key0 = deriver.derive_next();
        assert_eq!(deriver.current_index(), 1);

        let key1 = deriver.derive_next();
        assert_eq!(deriver.current_index(), 2);

        // Verify consistency with direct derivation
        let fresh_deriver = HdDeriver::new(seed);
        assert_eq!(
            key0.public_key().to_bytes(),
            fresh_deriver.derive(0).public_key().to_bytes()
        );
        assert_eq!(
            key1.public_key().to_bytes(),
            fresh_deriver.derive(1).public_key().to_bytes()
        );
    }

    #[test]
    fn test_different_passphrases() {
        let mnemonic = Mnemonic::generate();

        let seed1 = mnemonic.to_seed("");
        let seed2 = mnemonic.to_seed("password");

        let deriver1 = HdDeriver::new(seed1);
        let deriver2 = HdDeriver::new(seed2);

        let key1 = deriver1.derive(0);
        let key2 = deriver2.derive(0);

        assert_ne!(key1.public_key().to_bytes(), key2.public_key().to_bytes());
    }
}
