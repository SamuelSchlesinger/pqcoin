//! Wallet file storage.

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::keys::{EncryptedKey, KeyPair, WalletError};

/// Wallet file format version.
pub const WALLET_VERSION: u32 = 1;

/// Wallet file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletFile {
    /// File format version.
    pub version: u32,
    /// Wallet name.
    pub name: String,
    /// Encrypted secret key.
    pub encrypted_key: EncryptedKey,
    /// Public key (hex-encoded).
    pub public_key: String,
    /// Address (hex-encoded public key hash).
    pub address: String,
}

impl WalletFile {
    /// Create a new wallet file from a keypair.
    pub fn new(name: &str, keypair: &KeyPair, password: &str) -> Result<Self, WalletError> {
        let encrypted_key = EncryptedKey::encrypt(keypair.secret_key(), password)?;

        Ok(Self {
            version: WALLET_VERSION,
            name: name.to_string(),
            encrypted_key,
            public_key: hex::encode(keypair.public_key().to_bytes()),
            address: keypair.address().to_hex(),
        })
    }

    /// Save the wallet file to disk.
    pub fn save(&self, path: &Path) -> Result<(), WalletError> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| WalletError::Io(format!("failed to create directory: {e}")))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| WalletError::InvalidFormat(format!("serialization failed: {e}")))?;

        std::fs::write(path, json).map_err(|e| WalletError::Io(format!("write failed: {e}")))?;

        Ok(())
    }

    /// Load a wallet file from disk.
    pub fn load(path: &Path) -> Result<Self, WalletError> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| WalletError::Io(format!("read failed: {e}")))?;

        let wallet: Self = serde_json::from_str(&json)
            .map_err(|e| WalletError::InvalidFormat(format!("parse failed: {e}")))?;

        if wallet.version != WALLET_VERSION {
            return Err(WalletError::InvalidFormat(format!(
                "unsupported wallet version: {} (expected {})",
                wallet.version, WALLET_VERSION
            )));
        }

        Ok(wallet)
    }

    /// Decrypt the wallet and return the keypair.
    pub fn decrypt(&self, password: &str) -> Result<KeyPair, WalletError> {
        let secret_key = self.encrypted_key.decrypt(password)?;

        let public_key_bytes = hex::decode(&self.public_key)
            .map_err(|e| WalletError::InvalidFormat(format!("invalid public key hex: {e}")))?;

        let public_key = crate::crypto::PublicKey::from_bytes(&public_key_bytes)
            .ok_or_else(|| WalletError::InvalidFormat("invalid public key".into()))?;

        Ok(KeyPair::from_keys(public_key, secret_key))
    }
}

/// Wallet storage abstraction for file operations.
pub struct WalletStorage {
    /// Base directory for wallet files.
    base_dir: std::path::PathBuf,
}

impl WalletStorage {
    /// Create a new wallet storage with the default base directory.
    pub fn new() -> Self {
        let base_dir = dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".pqcoin");
        Self { base_dir }
    }

    /// Create a new wallet storage with a custom base directory.
    pub fn with_base_dir(base_dir: std::path::PathBuf) -> Self {
        Self { base_dir }
    }

    /// Get the path for the default wallet.
    pub fn default_wallet_path(&self) -> std::path::PathBuf {
        self.base_dir.join("wallet.json")
    }

    /// Get the path for a named wallet.
    pub fn wallet_path(&self, name: &str) -> std::path::PathBuf {
        self.base_dir.join(format!("{name}.wallet.json"))
    }

    /// List all wallet files in the storage directory.
    pub fn list_wallets(&self) -> Result<Vec<String>, WalletError> {
        if !self.base_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&self.base_dir)
            .map_err(|e| WalletError::Io(format!("failed to read directory: {e}")))?;

        let mut wallets = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| WalletError::Io(format!("failed to read entry: {e}")))?;
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".wallet.json") {
                    wallets.push(name.trim_end_matches(".wallet.json").to_string());
                } else if name == "wallet.json" {
                    wallets.push("default".to_string());
                }
            }
        }

        Ok(wallets)
    }

    /// Check if a wallet exists.
    pub fn wallet_exists(&self, name: &str) -> bool {
        if name == "default" {
            self.default_wallet_path().exists()
        } else {
            self.wallet_path(name).exists()
        }
    }
}

impl Default for WalletStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_wallet_file_roundtrip() {
        let keypair = KeyPair::generate();
        let password = "test-password";
        let name = "test-wallet";

        let wallet_file = WalletFile::new(name, &keypair, password).unwrap();

        let dir = tempdir().unwrap();
        let path = dir.path().join("test-wallet.json");

        wallet_file.save(&path).unwrap();

        let loaded = WalletFile::load(&path).unwrap();
        assert_eq!(loaded.name, name);
        assert_eq!(loaded.version, WALLET_VERSION);

        let decrypted = loaded.decrypt(password).unwrap();
        assert_eq!(
            decrypted.public_key().to_bytes(),
            keypair.public_key().to_bytes()
        );
    }

    #[test]
    fn test_wallet_storage_list() {
        let dir = tempdir().unwrap();
        let storage = WalletStorage::with_base_dir(dir.path().to_path_buf());

        // Initially empty
        assert!(storage.list_wallets().unwrap().is_empty());

        // Create a wallet
        let keypair = KeyPair::generate();
        let wallet_file = WalletFile::new("test", &keypair, "password").unwrap();
        wallet_file.save(&storage.wallet_path("test")).unwrap();

        // Should now list one wallet
        let wallets = storage.list_wallets().unwrap();
        assert_eq!(wallets.len(), 1);
        assert!(wallets.contains(&"test".to_string()));
    }
}
