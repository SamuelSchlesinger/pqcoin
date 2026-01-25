//! Cryptographic primitives for the pqcoin protocol.
//!
//! This module provides the core cryptographic building blocks:
//!
//! - **Hashing**: SHA3-512 ([FIPS 202]) for transaction and block hashing
//! - **Signatures**: ML-DSA-87 ([FIPS 204]) for post-quantum digital signatures
//!
//! # Design Principles
//!
//! All types expose a simple, byte-oriented API to isolate the rest of the codebase
//! from implementation details. This allows swapping cryptographic backends without
//! affecting dependent code.
//!
//! # Example
//!
//! ```
//! use pqcoin::crypto::{hash, ml_dsa_87};
//!
//! // Hash some data
//! let digest = hash(b"hello world");
//! println!("SHA3-512: {}", digest);
//!
//! // Generate keys and sign a message
//! let (public_key, secret_key) = ml_dsa_87::keygen();
//! let signature = ml_dsa_87::sign(&secret_key, b"transaction data");
//! assert!(ml_dsa_87::verify(&public_key, b"transaction data", &signature));
//! ```
//!
//! [FIPS 202]: https://csrc.nist.gov/publications/detail/fips/202/final
//! [FIPS 204]: https://csrc.nist.gov/publications/detail/fips/204/final

use sha3::{Digest, Sha3_512 as Sha3_512Hasher};

/// A SHA3-512 hash digest (64 bytes / 512 bits).
///
/// This type wraps the raw hash output and provides convenience methods for
/// serialization and display. It implements `Copy` for efficient pass-by-value.
///
/// # Example
///
/// ```
/// use pqcoin::crypto::hash;
///
/// let digest = hash(b"example");
/// assert_eq!(digest.as_bytes().len(), 64);
/// println!("{}", digest.to_hex());
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hash([u8; 64]);

impl Hash {
    /// The size of the hash in bytes.
    pub const SIZE: usize = 64;

    /// Create a hash from raw bytes.
    pub fn from_bytes(bytes: [u8; 64]) -> Self {
        Self(bytes)
    }

    /// Get the hash as a byte slice.
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
    }

    /// Convert to a hex string.
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl std::fmt::Debug for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Hash({})", self.to_hex())
    }
}

impl std::fmt::Display for Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl AsRef<[u8]> for Hash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Compute the SHA3-512 hash of the given data.
pub fn hash(data: &[u8]) -> Hash {
    let mut hasher = Sha3_512Hasher::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 64];
    output.copy_from_slice(&result);
    Hash(output)
}

/// Compute the SHA3-512 hash of multiple data slices.
pub fn hash_many(data: &[&[u8]]) -> Hash {
    let mut hasher = Sha3_512Hasher::new();
    for d in data {
        hasher.update(d);
    }
    let result = hasher.finalize();
    let mut output = [0u8; 64];
    output.copy_from_slice(&result);
    Hash(output)
}

/// ML-DSA-87 post-quantum digital signatures ([FIPS 204]).
///
/// ML-DSA-87 (formerly Dilithium5) provides post-quantum security at NIST security
/// level 5, offering ~256-bit classical and ~128-bit quantum security. This is the
/// highest security level in the ML-DSA family.
///
/// # Key Sizes
///
/// | Component   | Size (bytes) |
/// |-------------|--------------|
/// | Public key  | 2,592        |
/// | Secret key  | 4,896        |
/// | Signature   | 4,627        |
///
/// # Example
///
/// ```
/// use pqcoin::crypto::ml_dsa_87::{keygen, sign, verify};
///
/// let (pk, sk) = keygen();
/// let message = b"transfer 100 coins";
/// let sig = sign(&sk, message);
///
/// assert!(verify(&pk, message, &sig));
/// assert!(!verify(&pk, b"tampered message", &sig));
/// ```
///
/// [FIPS 204]: https://csrc.nist.gov/publications/detail/fips/204/final
pub mod ml_dsa_87 {
    use pqcrypto_dilithium::dilithium5;
    use pqcrypto_traits::sign::{
        DetachedSignature, PublicKey as PubKeyTrait, SecretKey as SecKeyTrait,
    };

    /// An ML-DSA-87 public key (2,592 bytes).
    ///
    /// Used to verify signatures created with the corresponding [`SecretKey`].
    /// Public keys can be freely shared and are safe to publish.
    #[derive(Clone)]
    pub struct PublicKey(dilithium5::PublicKey);

    impl std::fmt::Debug for PublicKey {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "PublicKey({}...)", hex::encode(&self.0.as_bytes()[..8]))
        }
    }

    impl PartialEq for PublicKey {
        fn eq(&self, other: &Self) -> bool {
            self.0.as_bytes() == other.0.as_bytes()
        }
    }

    impl Eq for PublicKey {}

    impl PublicKey {
        /// Serialize the public key to bytes.
        pub fn to_bytes(&self) -> Vec<u8> {
            self.0.as_bytes().to_vec()
        }

        /// Deserialize a public key from bytes.
        pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
            dilithium5::PublicKey::from_bytes(bytes).ok().map(Self)
        }

        /// Get the size of the public key in bytes.
        pub const fn size() -> usize {
            2592
        }
    }

    impl AsRef<[u8]> for PublicKey {
        fn as_ref(&self) -> &[u8] {
            self.0.as_bytes()
        }
    }

    /// An ML-DSA-87 secret key (4,896 bytes).
    ///
    /// Used to create signatures that can be verified with the corresponding [`PublicKey`].
    /// Secret keys must be kept confidential.
    #[derive(Clone)]
    pub struct SecretKey(dilithium5::SecretKey);

    impl SecretKey {
        /// Serialize the secret key to bytes.
        pub fn to_bytes(&self) -> Vec<u8> {
            self.0.as_bytes().to_vec()
        }

        /// Deserialize a secret key from bytes.
        pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
            dilithium5::SecretKey::from_bytes(bytes).ok().map(Self)
        }

        /// Get the size of the secret key in bytes.
        pub const fn size() -> usize {
            4896
        }
    }

    /// An ML-DSA-87 detached signature (4,627 bytes).
    ///
    /// A detached signature does not include the original message, so the message
    /// must be provided separately during verification.
    #[derive(Clone)]
    pub struct Signature(dilithium5::DetachedSignature);

    impl std::fmt::Debug for Signature {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "Signature({}...)", hex::encode(&self.0.as_bytes()[..8]))
        }
    }

    impl PartialEq for Signature {
        fn eq(&self, other: &Self) -> bool {
            self.0.as_bytes() == other.0.as_bytes()
        }
    }

    impl Eq for Signature {}

    impl Signature {
        /// Serialize the signature to bytes.
        pub fn to_bytes(&self) -> Vec<u8> {
            self.0.as_bytes().to_vec()
        }

        /// Deserialize a signature from bytes.
        pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
            dilithium5::DetachedSignature::from_bytes(bytes)
                .ok()
                .map(Self)
        }

        /// Get the size of the signature in bytes.
        pub const fn size() -> usize {
            4627
        }
    }

    impl AsRef<[u8]> for Signature {
        fn as_ref(&self) -> &[u8] {
            self.0.as_bytes()
        }
    }

    /// Generate a new ML-DSA-87 key pair.
    ///
    /// Returns a tuple of (public key, secret key). The keys are generated using
    /// a cryptographically secure random number generator.
    ///
    /// # Example
    ///
    /// ```
    /// use pqcoin::crypto::ml_dsa_87::keygen;
    ///
    /// let (public_key, secret_key) = keygen();
    /// ```
    pub fn keygen() -> (PublicKey, SecretKey) {
        let (pk, sk) = dilithium5::keypair();
        (PublicKey(pk), SecretKey(sk))
    }

    /// Sign a message with the secret key, returning a detached signature.
    ///
    /// The signature does not contain the message, so the original message must
    /// be retained for verification.
    ///
    /// # Example
    ///
    /// ```
    /// use pqcoin::crypto::ml_dsa_87::{keygen, sign};
    ///
    /// let (_, secret_key) = keygen();
    /// let signature = sign(&secret_key, b"message to sign");
    /// ```
    pub fn sign(sk: &SecretKey, message: &[u8]) -> Signature {
        Signature(dilithium5::detached_sign(message, &sk.0))
    }

    /// Verify a detached signature on a message with the public key.
    ///
    /// Returns `true` if the signature is valid for the given message and public key,
    /// `false` otherwise.
    ///
    /// # Example
    ///
    /// ```
    /// use pqcoin::crypto::ml_dsa_87::{keygen, sign, verify};
    ///
    /// let (public_key, secret_key) = keygen();
    /// let message = b"important message";
    /// let signature = sign(&secret_key, message);
    ///
    /// assert!(verify(&public_key, message, &signature));
    /// ```
    pub fn verify(pk: &PublicKey, message: &[u8], signature: &Signature) -> bool {
        dilithium5::verify_detached_signature(&signature.0, message, &pk.0).is_ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_keygen() {
            let (pk, sk) = keygen();
            assert_eq!(pk.to_bytes().len(), PublicKey::size());
            assert_eq!(sk.to_bytes().len(), SecretKey::size());
        }

        #[test]
        fn test_sign_verify() {
            let (pk, sk) = keygen();
            let message = b"Hello, post-quantum world!";
            let signature = sign(&sk, message);

            assert!(verify(&pk, message, &signature));
            assert_eq!(signature.to_bytes().len(), Signature::size());
        }

        #[test]
        fn test_sign_verify_wrong_message() {
            let (pk, sk) = keygen();
            let message = b"Hello, post-quantum world!";
            let signature = sign(&sk, message);

            let wrong_message = b"Hello, quantum world!";
            assert!(!verify(&pk, wrong_message, &signature));
        }

        #[test]
        fn test_serialization_roundtrip() {
            let (pk, sk) = keygen();
            let message = b"Test message";
            let signature = sign(&sk, message);

            // Roundtrip public key
            let pk_bytes = pk.to_bytes();
            let pk2 = PublicKey::from_bytes(&pk_bytes).unwrap();
            assert!(verify(&pk2, message, &signature));

            // Roundtrip signature
            let sig_bytes = signature.to_bytes();
            let sig2 = Signature::from_bytes(&sig_bytes).unwrap();
            assert!(verify(&pk, message, &sig2));
        }

        #[test]
        fn test_different_keys() {
            let (pk1, sk1) = keygen();
            let (pk2, _sk2) = keygen();

            let message = b"Test message";
            let signature = sign(&sk1, message);

            // Signature should verify with correct key
            assert!(verify(&pk1, message, &signature));
            // Signature should NOT verify with different key
            assert!(!verify(&pk2, message, &signature));
        }
    }
}

/// Re-export commonly used types.
pub use ml_dsa_87::{PublicKey, SecretKey, Signature};

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // SHA3-512 Hashing Tests
    // ========================================================================

    #[test]
    fn hash_empty_matches_nist_vector() {
        // SHA3-512("") from NIST FIPS 202 test vectors
        let h = hash(b"");
        let expected = "a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26";
        assert_eq!(h.to_hex(), expected);
    }

    #[test]
    fn hash_abc_matches_nist_vector() {
        // SHA3-512("abc") from NIST FIPS 202 test vectors
        let h = hash(b"abc");
        let expected = "b751850b1a57168a5693cd924b6b096e08f621827444f70d884f5d0240d2712e10e116e9192af3c91a7ec57647e3934057340b4cf408d5a56592f8274eec53f0";
        assert_eq!(h.to_hex(), expected);
    }

    #[test]
    fn hash_many_equivalent_to_concatenated() {
        let h1 = hash(b"abc");
        let h2 = hash_many(&[b"a", b"b", b"c"]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_output_is_64_bytes() {
        let h = hash(b"test");
        assert_eq!(h.as_bytes().len(), Hash::SIZE);
        assert_eq!(Hash::SIZE, 64);
    }

    #[test]
    fn hash_different_inputs_produce_different_outputs() {
        let h1 = hash(b"hello");
        let h2 = hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_same_input_produces_same_output() {
        let h1 = hash(b"deterministic");
        let h2 = hash(b"deterministic");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_from_bytes_roundtrip() {
        let original = hash(b"roundtrip test");
        let bytes = *original.as_bytes();
        let restored = Hash::from_bytes(bytes);
        assert_eq!(original, restored);
    }

    #[test]
    fn hash_display_format() {
        let h = hash(b"");
        let display = format!("{}", h);
        let debug = format!("{:?}", h);

        // Display should be just the hex
        assert_eq!(display.len(), 128); // 64 bytes = 128 hex chars
        assert!(display.chars().all(|c| c.is_ascii_hexdigit()));

        // Debug should include "Hash(...)"
        assert!(debug.starts_with("Hash("));
        assert!(debug.ends_with(")"));
    }

    #[test]
    fn hash_as_ref_returns_bytes() {
        let h = hash(b"test");
        let slice: &[u8] = h.as_ref();
        assert_eq!(slice.len(), 64);
        assert_eq!(slice, h.as_bytes());
    }
}
