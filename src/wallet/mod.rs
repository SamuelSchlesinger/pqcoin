//! Wallet functionality for pqcoin.
//!
//! This module provides:
//! - Key generation and management
//! - Encrypted wallet storage
//! - Transaction building and signing
//! - RPC client for node interaction
//! - Watch-only wallet support for cold storage workflows
//! - HD (Hierarchical Deterministic) wallet support

pub mod hd;
mod keys;
pub mod mnemonic;
pub mod psbt;
mod storage;
mod tx_builder;

pub use hd::HdDeriver;
pub use keys::{EncryptedKey, KeyPair, WalletError};
pub use mnemonic::{MasterSeed, Mnemonic};
pub use psbt::PartialTransaction;
pub use storage::{WalletFile, WalletStorage, WalletType};
pub use tx_builder::{TransactionBuilder, UtxoInput};

use crate::blockchain::{Address, Transaction, Witness};
use crate::crypto::{PublicKey, hash};

/// A pqcoin wallet.
#[derive(Debug)]
pub struct Wallet {
    /// Wallet name.
    pub name: String,
    /// The wallet's keypair (if unlocked).
    keypair: Option<KeyPair>,
    /// The wallet's public key (if available - not for address-only wallets).
    public_key: Option<PublicKey>,
    /// The wallet's address (primary/current address).
    pub address: Address,
    /// Path to the wallet file.
    pub path: std::path::PathBuf,
    /// Whether this is a watch-only wallet.
    watch_only: bool,
    /// Whether this is an HD wallet.
    is_hd: bool,
    /// HD deriver (if unlocked and this is an HD wallet).
    hd_deriver: Option<HdDeriver>,
}

impl Wallet {
    /// Create a new wallet with a freshly generated keypair.
    pub fn create(
        name: &str,
        password: &str,
        path: std::path::PathBuf,
    ) -> Result<Self, WalletError> {
        let keypair = KeyPair::generate();
        let address = keypair.address();
        let public_key = keypair.public_key().clone();

        let wallet_file = WalletFile::new(name, &keypair, password)?;
        wallet_file.save(&path)?;

        Ok(Self {
            name: name.to_string(),
            keypair: Some(keypair),
            public_key: Some(public_key),
            address,
            path,
            watch_only: false,
            is_hd: false,
            hd_deriver: None,
        })
    }

    /// Create a new HD wallet from a mnemonic.
    ///
    /// The master seed derived from the mnemonic is encrypted and stored.
    /// Returns the mnemonic so the user can back it up.
    pub fn create_hd(
        name: &str,
        password: &str,
        path: std::path::PathBuf,
    ) -> Result<(Self, Mnemonic), WalletError> {
        // Generate a new mnemonic
        let mnemonic = Mnemonic::generate();
        let seed = mnemonic.to_seed("");
        let deriver = HdDeriver::new(seed.clone());

        // Derive the first keypair
        let keypair = deriver.derive(0);
        let address = keypair.address();
        let public_key = keypair.public_key().clone();

        // Create and save the HD wallet file
        let wallet_file = WalletFile::new_hd(name, seed.as_bytes(), &keypair, password)?;
        wallet_file.save(&path)?;

        Ok((
            Self {
                name: name.to_string(),
                keypair: Some(keypair),
                public_key: Some(public_key),
                address,
                path,
                watch_only: false,
                is_hd: true,
                hd_deriver: Some(deriver),
            },
            mnemonic,
        ))
    }

    /// Recover an HD wallet from a mnemonic phrase.
    pub fn recover_hd(
        name: &str,
        mnemonic: &Mnemonic,
        passphrase: &str,
        password: &str,
        path: std::path::PathBuf,
    ) -> Result<Self, WalletError> {
        let seed = mnemonic.to_seed(passphrase);
        let deriver = HdDeriver::new(seed.clone());

        // Derive the first keypair
        let keypair = deriver.derive(0);
        let address = keypair.address();
        let public_key = keypair.public_key().clone();

        // Create and save the HD wallet file
        let wallet_file = WalletFile::new_hd(name, seed.as_bytes(), &keypair, password)?;
        wallet_file.save(&path)?;

        Ok(Self {
            name: name.to_string(),
            keypair: Some(keypair),
            public_key: Some(public_key),
            address,
            path,
            watch_only: false,
            is_hd: true,
            hd_deriver: Some(deriver),
        })
    }

    /// Create a watch-only wallet from a public key.
    ///
    /// Watch-only wallets can monitor balances and create unsigned transactions,
    /// but cannot sign. This enables cold storage workflows where signing happens offline.
    pub fn create_watch_only(
        name: &str,
        public_key: &PublicKey,
        path: std::path::PathBuf,
    ) -> Result<Self, WalletError> {
        let address = Address::from_public_key(public_key);

        let wallet_file = WalletFile::new_watch_only(name, public_key);
        wallet_file.save(&path)?;

        Ok(Self {
            name: name.to_string(),
            keypair: None,
            public_key: Some(public_key.clone()),
            address,
            path,
            watch_only: true,
            is_hd: false,
            hd_deriver: None,
        })
    }

    /// Create an address-only watch wallet.
    ///
    /// This wallet can only check balance; it cannot create transactions
    /// because the public key is not known.
    pub fn create_watch_only_address(
        name: &str,
        address: &Address,
        path: std::path::PathBuf,
    ) -> Result<Self, WalletError> {
        let wallet_file = WalletFile::new_watch_only_address(name, address);
        wallet_file.save(&path)?;

        Ok(Self {
            name: name.to_string(),
            keypair: None,
            public_key: None,
            address: *address,
            path,
            watch_only: true,
            is_hd: false,
            hd_deriver: None,
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

        let public_key = wallet_file.get_public_key();
        let watch_only = wallet_file.is_watch_only();
        let is_hd = wallet_file.is_hd();

        Ok(Self {
            name: wallet_file.name,
            keypair: None, // Not unlocked yet
            public_key,
            address,
            path,
            watch_only,
            is_hd,
            hd_deriver: None, // Will be set when unlocked
        })
    }

    /// Unlock the wallet with a password.
    ///
    /// Returns `WalletError::WatchOnly` for watch-only wallets.
    pub fn unlock(&mut self, password: &str) -> Result<(), WalletError> {
        if self.watch_only {
            return Err(WalletError::WatchOnly);
        }

        let wallet_file = WalletFile::load(&self.path)?;
        let keypair = wallet_file.decrypt(password)?;
        self.public_key = Some(keypair.public_key().clone());
        self.keypair = Some(keypair);

        // For HD wallets, also decrypt the seed and create the deriver
        if self.is_hd {
            let seed_bytes = wallet_file.decrypt_seed(password)?;
            if seed_bytes.len() >= 64 {
                let mut seed_arr = [0u8; 64];
                seed_arr.copy_from_slice(&seed_bytes[..64]);
                let seed = MasterSeed::from_bytes(seed_arr);
                self.hd_deriver = Some(HdDeriver::new(seed));
            }
        }

        Ok(())
    }

    /// Lock the wallet (clear the keypair from memory).
    pub fn lock(&mut self) {
        self.keypair = None;
        self.hd_deriver = None;
    }

    /// Check if the wallet is unlocked.
    pub fn is_unlocked(&self) -> bool {
        self.keypair.is_some()
    }

    /// Check if this is a watch-only wallet.
    pub fn is_watch_only(&self) -> bool {
        self.watch_only
    }

    /// Check if this is an HD wallet.
    pub fn is_hd(&self) -> bool {
        self.is_hd
    }

    /// Check if this wallet has a public key (not address-only).
    pub fn has_public_key(&self) -> bool {
        self.public_key.is_some()
    }

    /// Get the keypair (if unlocked).
    pub fn keypair(&self) -> Option<&KeyPair> {
        self.keypair.as_ref()
    }

    /// Get the public key (if available).
    pub fn public_key(&self) -> Option<&PublicKey> {
        self.public_key.as_ref()
    }

    /// Get the wallet's primary address.
    pub fn address(&self) -> &Address {
        &self.address
    }

    /// Derive a new address for HD wallets.
    ///
    /// This derives the next address and updates the wallet file.
    /// Returns the new address and its derivation index.
    pub fn derive_new_address(&mut self, password: &str) -> Result<(Address, u32), WalletError> {
        if !self.is_hd {
            return Err(WalletError::InvalidFormat(
                "not an HD wallet - use an HD wallet to derive new addresses".into(),
            ));
        }

        // Load and update the wallet file
        let mut wallet_file = WalletFile::load(&self.path)?;
        let hd_meta = wallet_file
            .hd_metadata_mut()
            .ok_or_else(|| WalletError::InvalidFormat("missing HD metadata".into()))?;

        let next_index = hd_meta.next_index;

        // We need the deriver to derive the new key
        let deriver = if let Some(ref d) = self.hd_deriver {
            d.clone()
        } else {
            // Need to decrypt seed
            let seed_bytes = wallet_file.decrypt_seed(password)?;
            if seed_bytes.len() < 64 {
                return Err(WalletError::InvalidFormat("invalid seed length".into()));
            }
            let mut seed_arr = [0u8; 64];
            seed_arr.copy_from_slice(&seed_bytes[..64]);
            HdDeriver::new(MasterSeed::from_bytes(seed_arr))
        };

        // Derive the new keypair
        let keypair = deriver.derive(next_index);
        let new_address = keypair.address();

        // Update the metadata
        let hd_meta = wallet_file.hd_metadata_mut().unwrap();
        hd_meta.next_index = next_index + 1;
        hd_meta.derived_addresses.push(new_address.to_hex());

        // Update the current address and keypair
        wallet_file.address = new_address.to_hex();
        wallet_file.public_key = Some(hex::encode(keypair.public_key().to_bytes()));

        // Re-encrypt the new key
        wallet_file.encrypted_key = Some(EncryptedKey::encrypt(keypair.secret_key(), password)?);

        // Save the updated wallet
        wallet_file.save(&self.path)?;

        // Update our state
        self.address = new_address;
        self.public_key = Some(keypair.public_key().clone());
        self.keypair = Some(keypair);

        Ok((new_address, next_index))
    }

    /// Get all derived addresses for an HD wallet.
    pub fn derived_addresses(&self) -> Result<Vec<String>, WalletError> {
        if !self.is_hd {
            return Ok(vec![self.address.to_hex()]);
        }

        let wallet_file = WalletFile::load(&self.path)?;
        let hd_meta = wallet_file
            .hd_metadata()
            .ok_or_else(|| WalletError::InvalidFormat("missing HD metadata".into()))?;

        Ok(hd_meta.derived_addresses.clone())
    }

    /// Get the next derivation index for an HD wallet.
    pub fn next_derivation_index(&self) -> Result<u32, WalletError> {
        if !self.is_hd {
            return Err(WalletError::InvalidFormat("not an HD wallet".into()));
        }

        let wallet_file = WalletFile::load(&self.path)?;
        let hd_meta = wallet_file
            .hd_metadata()
            .ok_or_else(|| WalletError::InvalidFormat("missing HD metadata".into()))?;

        Ok(hd_meta.next_index)
    }

    /// Sign a transaction using this wallet's keypair.
    ///
    /// This method takes an unsigned transaction and signs all inputs that
    /// belong to this wallet's address.
    ///
    /// # Errors
    ///
    /// Returns `WalletError::WatchOnly` if this is a watch-only wallet.
    /// Returns `WalletError::Locked` if the wallet is not unlocked.
    pub fn sign_transaction(&self, tx: &mut Transaction) -> Result<(), WalletError> {
        if self.watch_only {
            return Err(WalletError::WatchOnly);
        }

        let keypair = self.keypair.as_ref().ok_or(WalletError::Locked)?;

        // Sign each input
        for i in 0..tx.inputs.len() {
            let signing_data = tx.signing_data(i);
            let message_hash = hash(&signing_data);
            let signature = keypair.sign(message_hash.as_bytes());

            tx.inputs[i].witness = Witness::P2PKH {
                public_key: Box::new(keypair.public_key().clone()),
                signature: Box::new(signature),
            };
        }

        Ok(())
    }

    /// Get the default wallet path.
    pub fn default_path() -> std::path::PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join(".pqcoin")
            .join("wallet.json")
    }
}
