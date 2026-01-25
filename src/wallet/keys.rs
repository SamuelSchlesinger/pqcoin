//! Key management and encryption for pqcoin wallets.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::blockchain::Address;
use crate::crypto::{PublicKey, SecretKey, ml_dsa_87};

/// Wallet errors.
#[derive(Debug, Clone)]
pub enum WalletError {
    /// I/O error.
    Io(String),
    /// Encryption/decryption error.
    Crypto(String),
    /// Invalid wallet format.
    InvalidFormat(String),
    /// Wallet is locked.
    Locked,
    /// RPC error.
    Rpc(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletError::Io(e) => write!(f, "I/O error: {e}"),
            WalletError::Crypto(e) => write!(f, "crypto error: {e}"),
            WalletError::InvalidFormat(e) => write!(f, "invalid format: {e}"),
            WalletError::Locked => write!(f, "wallet is locked"),
            WalletError::Rpc(e) => write!(f, "RPC error: {e}"),
        }
    }
}

impl std::error::Error for WalletError {}

/// A keypair for signing transactions.
#[derive(Clone)]
pub struct KeyPair {
    /// The public key.
    pub public_key: PublicKey,
    /// The secret key.
    secret_key: SecretKey,
}

impl KeyPair {
    /// Generate a new random keypair.
    pub fn generate() -> Self {
        let (public_key, secret_key) = ml_dsa_87::keygen();
        Self {
            public_key,
            secret_key,
        }
    }

    /// Create a keypair from existing keys.
    pub fn from_keys(public_key: PublicKey, secret_key: SecretKey) -> Self {
        Self {
            public_key,
            secret_key,
        }
    }

    /// Get the public key.
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Get the secret key.
    pub fn secret_key(&self) -> &SecretKey {
        &self.secret_key
    }

    /// Get the address derived from the public key.
    pub fn address(&self) -> Address {
        Address::from_public_key(&self.public_key)
    }

    /// Sign a message.
    pub fn sign(&self, message: &[u8]) -> crate::crypto::Signature {
        ml_dsa_87::sign(&self.secret_key, message)
    }
}

impl std::fmt::Debug for KeyPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeyPair")
            .field("address", &self.address())
            .finish_non_exhaustive()
    }
}

/// Encrypted secret key stored in the wallet file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedKey {
    /// The encrypted secret key (base64-encoded).
    pub ciphertext: String,
    /// The nonce used for encryption (base64-encoded).
    pub nonce: String,
    /// The salt used for key derivation (base64-encoded).
    pub salt: String,
}

impl EncryptedKey {
    /// Encrypt a secret key with a password.
    pub fn encrypt(secret_key: &SecretKey, password: &str) -> Result<Self, WalletError> {
        // Generate random salt and nonce
        let mut salt_bytes = [0u8; 16];
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut salt_bytes);
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        // Derive encryption key from password using Argon2
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|e| WalletError::Crypto(format!("salt encoding failed: {e}")))?;

        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| WalletError::Crypto(format!("key derivation failed: {e}")))?;

        // Extract the 32-byte hash output
        let hash_output = password_hash
            .hash
            .ok_or_else(|| WalletError::Crypto("no hash output".into()))?;
        let hash_bytes = hash_output.as_bytes();

        // Use first 32 bytes as AES-256 key
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash_bytes[..32]);

        // Encrypt the secret key
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| WalletError::Crypto(format!("cipher init failed: {e}")))?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, secret_key.to_bytes().as_ref())
            .map_err(|e| WalletError::Crypto(format!("encryption failed: {e}")))?;

        // Zeroize the key
        key.zeroize();

        Ok(Self {
            ciphertext: base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &ciphertext,
            ),
            nonce: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, nonce_bytes),
            salt: salt.to_string(),
        })
    }

    /// Decrypt a secret key with a password.
    pub fn decrypt(&self, password: &str) -> Result<SecretKey, WalletError> {
        use base64::Engine;

        // Decode base64 values
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(&self.ciphertext)
            .map_err(|e| WalletError::Crypto(format!("ciphertext decode failed: {e}")))?;

        let nonce_bytes = base64::engine::general_purpose::STANDARD
            .decode(&self.nonce)
            .map_err(|e| WalletError::Crypto(format!("nonce decode failed: {e}")))?;

        // Derive encryption key from password using Argon2
        let salt = SaltString::from_b64(&self.salt)
            .map_err(|e| WalletError::Crypto(format!("salt decode failed: {e}")))?;

        let argon2 = Argon2::default();
        let password_hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| WalletError::Crypto(format!("key derivation failed: {e}")))?;

        // Extract the 32-byte hash output
        let hash_output = password_hash
            .hash
            .ok_or_else(|| WalletError::Crypto("no hash output".into()))?;
        let hash_bytes = hash_output.as_bytes();

        // Use first 32 bytes as AES-256 key
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash_bytes[..32]);

        // Decrypt the secret key
        let cipher = Aes256Gcm::new_from_slice(&key)
            .map_err(|e| WalletError::Crypto(format!("cipher init failed: {e}")))?;

        let nonce = Nonce::from_slice(&nonce_bytes);
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|_| WalletError::Crypto("decryption failed (wrong password?)".into()))?;

        // Zeroize the key
        key.zeroize();

        // Convert to secret key
        SecretKey::from_bytes(&plaintext)
            .ok_or_else(|| WalletError::Crypto("invalid secret key".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keypair_generation() {
        let keypair = KeyPair::generate();
        let address = keypair.address();
        assert!(!address.to_hex().is_empty());
    }

    #[test]
    fn test_encryption_roundtrip() {
        let keypair = KeyPair::generate();
        let password = "test-password-123";

        let encrypted = EncryptedKey::encrypt(keypair.secret_key(), password).unwrap();
        let decrypted = encrypted.decrypt(password).unwrap();

        assert_eq!(keypair.secret_key().to_bytes(), decrypted.to_bytes());
    }

    #[test]
    fn test_wrong_password() {
        let keypair = KeyPair::generate();
        let password = "correct-password";
        let wrong_password = "wrong-password";

        let encrypted = EncryptedKey::encrypt(keypair.secret_key(), password).unwrap();
        let result = encrypted.decrypt(wrong_password);

        assert!(result.is_err());
    }
}
