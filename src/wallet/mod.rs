//! Wallet functionality for pqcoin.
//!
//! This module provides:
//! - Key generation and management
//! - Encrypted wallet storage
//! - Transaction building and signing
//! - RPC client for node interaction

mod keys;
mod storage;
mod tx_builder;

pub use keys::{EncryptedKey, KeyPair, WalletError};
pub use storage::{WalletFile, WalletStorage};
pub use tx_builder::{TransactionBuilder, UtxoInput};

use crate::blockchain::Address;

/// A pqcoin wallet.
#[derive(Debug)]
pub struct Wallet {
    /// Wallet name.
    pub name: String,
    /// The wallet's keypair (if unlocked).
    keypair: Option<KeyPair>,
    /// The wallet's address.
    pub address: Address,
    /// Path to the wallet file.
    pub path: std::path::PathBuf,
}

impl Wallet {
    /// Create a new wallet with a freshly generated keypair.
    pub fn create(name: &str, password: &str, path: std::path::PathBuf) -> Result<Self, WalletError> {
        let keypair = KeyPair::generate();
        let address = keypair.address();

        let wallet_file = WalletFile::new(name, &keypair, password)?;
        wallet_file.save(&path)?;

        Ok(Self {
            name: name.to_string(),
            keypair: Some(keypair),
            address,
            path,
        })
    }

    /// Load a wallet from a file.
    pub fn load(path: std::path::PathBuf) -> Result<Self, WalletError> {
        let wallet_file = WalletFile::load(&path)?;
        let address = Address::from_hash(crate::crypto::Hash::from_bytes(
            hex::decode(&wallet_file.address)
                .map_err(|_| WalletError::InvalidFormat("invalid address hex".into()))?
                .try_into()
                .map_err(|_| WalletError::InvalidFormat("invalid address length".into()))?,
        ));

        Ok(Self {
            name: wallet_file.name,
            keypair: None, // Not unlocked yet
            address,
            path,
        })
    }

    /// Unlock the wallet with a password.
    pub fn unlock(&mut self, password: &str) -> Result<(), WalletError> {
        let wallet_file = WalletFile::load(&self.path)?;
        let keypair = wallet_file.decrypt(password)?;
        self.keypair = Some(keypair);
        Ok(())
    }

    /// Lock the wallet (clear the keypair from memory).
    pub fn lock(&mut self) {
        self.keypair = None;
    }

    /// Check if the wallet is unlocked.
    pub fn is_unlocked(&self) -> bool {
        self.keypair.is_some()
    }

    /// Get the keypair (if unlocked).
    pub fn keypair(&self) -> Option<&KeyPair> {
        self.keypair.as_ref()
    }

    /// Get the wallet's address.
    pub fn address(&self) -> &Address {
        &self.address
    }

    /// Get the default wallet path.
    pub fn default_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".pqcoin")
            .join("wallet.json")
    }
}
