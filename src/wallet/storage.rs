//! Wallet file storage.

use serde::{Deserialize, Serialize};
use std::path::Path;

use super::keys::{EncryptedKey, KeyPair, WalletError};
use crate::crypto::PublicKey;

/// Wallet file format version.
pub const WALLET_VERSION: u32 = 2;

/// Wallet type enumeration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WalletType {
    /// Standard wallet with single keypair.
    Standard,
    /// HD (Hierarchical Deterministic) wallet derived from mnemonic.
    Hd,
    /// Watch-only wallet (no secret key).
    WatchOnly,
}

impl Default for WalletType {
    fn default() -> Self {
        Self::Standard
    }
}

/// HD wallet metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdMetadata {
    /// Encrypted master seed (64 bytes, derived from mnemonic).
    pub encrypted_seed: EncryptedKey,
    /// Next derivation index.
    pub next_index: u32,
    /// List of derived addresses (for recovery/tracking).
    #[serde(default)]
    pub derived_addresses: Vec<String>,
}

/// Wallet file structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletFile {
    /// File format version.
    pub version: u32,
    /// Wallet name.
    pub name: String,
    /// Wallet type.
    #[serde(default)]
    pub wallet_type: WalletType,
    /// Encrypted secret key (None for watch-only and HD wallets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encrypted_key: Option<EncryptedKey>,
    /// Public key (hex-encoded). None for address-only watch wallets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    /// Address (hex-encoded public key hash).
    pub address: String,
    /// Whether this is a watch-only wallet (deprecated, use wallet_type).
    #[serde(default)]
    pub watch_only: bool,
    /// HD wallet metadata (only for HD wallets).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hd_metadata: Option<HdMetadata>,
}

impl WalletFile {
    /// Create a new wallet file from a keypair.
    pub fn new(name: &str, keypair: &KeyPair, password: &str) -> Result<Self, WalletError> {
        let encrypted_key = EncryptedKey::encrypt(keypair.secret_key(), password)?;

        Ok(Self {
            version: WALLET_VERSION,
            name: name.to_string(),
            wallet_type: WalletType::Standard,
            encrypted_key: Some(encrypted_key),
            public_key: Some(hex::encode(keypair.public_key().to_bytes())),
            address: keypair.address().to_hex(),
            watch_only: false,
            hd_metadata: None,
        })
    }

    /// Create a new HD wallet from a master seed.
    ///
    /// The master seed is encrypted and stored. The first address is derived
    /// automatically.
    pub fn new_hd(
        name: &str,
        master_seed: &[u8],
        first_keypair: &KeyPair,
        password: &str,
    ) -> Result<Self, WalletError> {
        // Encrypt the master seed using the same encryption as secret keys
        // We'll use a SecretKey wrapper for encryption since it handles the
        // AES-GCM encryption properly
        let encrypted_seed = EncryptedKey::encrypt_bytes(master_seed, password)?;

        // Also encrypt the first derived secret key for quick access
        let encrypted_key = EncryptedKey::encrypt(first_keypair.secret_key(), password)?;

        let first_address = first_keypair.address().to_hex();

        Ok(Self {
            version: WALLET_VERSION,
            name: name.to_string(),
            wallet_type: WalletType::Hd,
            encrypted_key: Some(encrypted_key),
            public_key: Some(hex::encode(first_keypair.public_key().to_bytes())),
            address: first_address.clone(),
            watch_only: false,
            hd_metadata: Some(HdMetadata {
                encrypted_seed,
                next_index: 1, // Already derived index 0
                derived_addresses: vec![first_address],
            }),
        })
    }

    /// Check if this is an HD wallet.
    pub fn is_hd(&self) -> bool {
        self.wallet_type == WalletType::Hd
    }

    /// Get the HD metadata if this is an HD wallet.
    pub fn hd_metadata(&self) -> Option<&HdMetadata> {
        self.hd_metadata.as_ref()
    }

    /// Get mutable HD metadata.
    pub fn hd_metadata_mut(&mut self) -> Option<&mut HdMetadata> {
        self.hd_metadata.as_mut()
    }

    /// Decrypt the master seed for HD wallets.
    pub fn decrypt_seed(&self, password: &str) -> Result<Vec<u8>, WalletError> {
        if self.wallet_type != WalletType::Hd {
            return Err(WalletError::InvalidFormat("not an HD wallet".into()));
        }

        let hd_meta = self
            .hd_metadata
            .as_ref()
            .ok_or_else(|| WalletError::InvalidFormat("missing HD metadata".into()))?;

        hd_meta.encrypted_seed.decrypt_bytes(password)
    }

    /// Create a watch-only wallet file from a public key.
    pub fn new_watch_only(name: &str, public_key: &PublicKey) -> Self {
        use crate::blockchain::Address;
        let address = Address::from_public_key(public_key);

        Self {
            version: WALLET_VERSION,
            name: name.to_string(),
            wallet_type: WalletType::WatchOnly,
            encrypted_key: None,
            public_key: Some(hex::encode(public_key.to_bytes())),
            address: address.to_hex(),
            watch_only: true,
            hd_metadata: None,
        }
    }

    /// Create an address-only watch wallet (can only check balance, not create transactions).
    pub fn new_watch_only_address(name: &str, address: &crate::blockchain::Address) -> Self {
        Self {
            version: WALLET_VERSION,
            name: name.to_string(),
            wallet_type: WalletType::WatchOnly,
            encrypted_key: None,
            public_key: None,
            address: address.to_hex(),
            watch_only: true,
            hd_metadata: None,
        }
    }

    /// Check if this is a watch-only wallet.
    pub fn is_watch_only(&self) -> bool {
        self.watch_only
    }

    /// Check if this wallet has a public key (not address-only).
    pub fn has_public_key(&self) -> bool {
        self.public_key.is_some()
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
    ///
    /// Returns `WalletError::WatchOnly` if this is a watch-only wallet.
    pub fn decrypt(&self, password: &str) -> Result<KeyPair, WalletError> {
        if self.watch_only {
            return Err(WalletError::WatchOnly);
        }

        let encrypted_key = self.encrypted_key.as_ref().ok_or(WalletError::WatchOnly)?;

        let secret_key = encrypted_key.decrypt(password)?;

        let public_key_hex = self
            .public_key
            .as_ref()
            .ok_or_else(|| WalletError::InvalidFormat("missing public key".into()))?;

        let public_key_bytes = hex::decode(public_key_hex)
            .map_err(|e| WalletError::InvalidFormat(format!("invalid public key hex: {e}")))?;

        let public_key = crate::crypto::PublicKey::from_bytes(&public_key_bytes)
            .ok_or_else(|| WalletError::InvalidFormat("invalid public key".into()))?;

        Ok(KeyPair::from_keys(public_key, secret_key))
    }

    /// Get the public key if available.
    pub fn get_public_key(&self) -> Option<PublicKey> {
        self.public_key.as_ref().and_then(|pk_hex| {
            hex::decode(pk_hex)
                .ok()
                .and_then(|bytes| PublicKey::from_bytes(&bytes))
        })
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
        assert!(!loaded.is_watch_only());

        let decrypted = loaded.decrypt(password).unwrap();
        assert_eq!(
            decrypted.public_key().to_bytes(),
            keypair.public_key().to_bytes()
        );
    }

    #[test]
    fn test_watch_only_wallet() {
        let keypair = KeyPair::generate();
        let name = "watch-only";

        let wallet_file = WalletFile::new_watch_only(name, keypair.public_key());

        let dir = tempdir().unwrap();
        let path = dir.path().join("watch-only.json");

        wallet_file.save(&path).unwrap();

        let loaded = WalletFile::load(&path).unwrap();
        assert_eq!(loaded.name, name);
        assert!(loaded.is_watch_only());
        assert!(loaded.has_public_key());

        // Decrypt should fail for watch-only wallet
        let result = loaded.decrypt("any-password");
        assert!(matches!(result, Err(WalletError::WatchOnly)));

        // Should be able to get public key
        let pk = loaded.get_public_key().unwrap();
        assert_eq!(pk.to_bytes(), keypair.public_key().to_bytes());
    }

    #[test]
    fn test_address_only_watch_wallet() {
        use crate::blockchain::Address;

        let keypair = KeyPair::generate();
        let address = Address::from_public_key(keypair.public_key());
        let name = "address-only";

        let wallet_file = WalletFile::new_watch_only_address(name, &address);

        let dir = tempdir().unwrap();
        let path = dir.path().join("address-only.json");

        wallet_file.save(&path).unwrap();

        let loaded = WalletFile::load(&path).unwrap();
        assert_eq!(loaded.name, name);
        assert!(loaded.is_watch_only());
        assert!(!loaded.has_public_key());

        // get_public_key should return None
        assert!(loaded.get_public_key().is_none());
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
