//! LMDB storage implementation using heed.

use std::fs;
use std::path::Path;

use heed::types::Bytes;
use heed::{Database, Env, EnvOpenOptions};

use super::codecs::{BlockCodec, HashCodec, OutPointCodec, StrCodec, U64Codec, UtxoCodec};
use super::error::StorageError;
use super::metadata_keys;
use super::{ChainConfig, StorageRead, StorageWrite, StorageWriteTxn};
use crate::blockchain::{Block, OutPoint, Utxo};
use crate::crypto::Hash;

/// Maximum number of named databases.
const MAX_DBS: u32 = 8;

/// Default map size (10 GB).
const DEFAULT_MAP_SIZE: usize = 10 * 1024 * 1024 * 1024;

/// LMDB-based persistent storage.
pub struct LmdbStorage {
    env: Env,
    blocks: Database<HashCodec, BlockCodec>,
    heights: Database<HashCodec, U64Codec>,
    utxos: Database<OutPointCodec, UtxoCodec>,
    tx_index: Database<HashCodec, HashCodec>,
    metadata: Database<StrCodec, Bytes>,
}

impl LmdbStorage {
    /// Open or create storage at the given path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref();

        // Create directory if it doesn't exist
        fs::create_dir_all(path).map_err(|e| StorageError::Open {
            path: path.to_path_buf(),
            error: e.to_string(),
        })?;

        // Open the LMDB environment
        //
        // SAFETY: The `heed` crate requires `unsafe` for `EnvOpenOptions::open()` because
        // LMDB has specific requirements about file locking and memory-mapped I/O:
        //
        // 1. Only one process should open a given LMDB environment at a time with write access.
        //    We ensure this by using a single `LmdbStorage` instance per node process.
        //
        // 2. The environment must not be opened multiple times in the same process.
        //    The `Blockchain::open()` function is called once during node initialization.
        //
        // 3. Memory-mapped regions must not be accessed after the environment is closed.
        //    The `Env` is stored in `LmdbStorage` and lives for the lifetime of the node.
        //
        // 4. The path must be a valid directory with appropriate permissions.
        //    We create the directory above with `fs::create_dir_all()`.
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(DEFAULT_MAP_SIZE)
                .max_dbs(MAX_DBS)
                .open(path)
                .map_err(|e| StorageError::Open {
                    path: path.to_path_buf(),
                    error: e.to_string(),
                })?
        };

        // Create or open databases
        let mut wtxn = env
            .write_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;

        let blocks = env
            .create_database(&mut wtxn, Some("blocks"))
            .map_err(|e| StorageError::CreateDb {
                name: "blocks",
                error: e.to_string(),
            })?;

        let heights = env
            .create_database(&mut wtxn, Some("heights"))
            .map_err(|e| StorageError::CreateDb {
                name: "heights",
                error: e.to_string(),
            })?;

        let utxos =
            env.create_database(&mut wtxn, Some("utxos"))
                .map_err(|e| StorageError::CreateDb {
                    name: "utxos",
                    error: e.to_string(),
                })?;

        let tx_index = env
            .create_database(&mut wtxn, Some("tx_index"))
            .map_err(|e| StorageError::CreateDb {
                name: "tx_index",
                error: e.to_string(),
            })?;

        let metadata = env
            .create_database(&mut wtxn, Some("metadata"))
            .map_err(|e| StorageError::CreateDb {
                name: "metadata",
                error: e.to_string(),
            })?;

        wtxn.commit()
            .map_err(|e| StorageError::Commit(e.to_string()))?;

        Ok(Self {
            env,
            blocks,
            heights,
            utxos,
            tx_index,
            metadata,
        })
    }

    /// Check if storage is initialized (has a genesis block).
    pub fn is_initialized(&self) -> Result<bool, StorageError> {
        Ok(self.get_genesis_hash()?.is_some())
    }

    /// Load chain configuration from storage.
    pub fn load_config(&self) -> Result<Option<ChainConfig>, StorageError> {
        let genesis_hash = match self.get_genesis_hash()? {
            Some(h) => h,
            None => return Ok(None),
        };

        let tip_hash = self.get_tip_hash()?.ok_or(StorageError::Corruption(
            "missing tip_hash in initialized storage".to_string(),
        ))?;

        let tip_height = self.get_tip_height()?.ok_or(StorageError::Corruption(
            "missing tip_height in initialized storage".to_string(),
        ))?;

        let difficulty_interval = self
            .get_metadata_u64(metadata_keys::DIFFICULTY_INTERVAL)?
            .ok_or(StorageError::Corruption(
                "missing difficulty_interval".to_string(),
            ))?;

        let target_block_time = self
            .get_metadata_u64(metadata_keys::TARGET_BLOCK_TIME)?
            .ok_or(StorageError::Corruption(
                "missing target_block_time".to_string(),
            ))?;

        let initial_reward = self
            .get_metadata_u64(metadata_keys::INITIAL_REWARD)?
            .ok_or(StorageError::Corruption(
                "missing initial_reward".to_string(),
            ))?;

        let halving_interval = self
            .get_metadata_u64(metadata_keys::HALVING_INTERVAL)?
            .ok_or(StorageError::Corruption(
                "missing halving_interval".to_string(),
            ))?;

        Ok(Some(ChainConfig {
            genesis_hash,
            tip_hash,
            tip_height,
            difficulty_interval,
            target_block_time,
            initial_reward,
            halving_interval,
        }))
    }

    /// Load all blocks from storage.
    pub fn load_all_blocks(&self) -> Result<Vec<(Hash, Block)>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        let mut blocks = Vec::new();

        let iter = self
            .blocks
            .iter(&rtxn)
            .map_err(|e| StorageError::Read(e.to_string()))?;

        for result in iter {
            let (hash, block) = result.map_err(|e| StorageError::Read(e.to_string()))?;
            blocks.push((hash, block));
        }

        Ok(blocks)
    }

    /// Load all block heights from storage.
    pub fn load_all_heights(&self) -> Result<Vec<(Hash, u64)>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        let mut heights = Vec::new();

        let iter = self
            .heights
            .iter(&rtxn)
            .map_err(|e| StorageError::Read(e.to_string()))?;

        for result in iter {
            let (hash, height) = result.map_err(|e| StorageError::Read(e.to_string()))?;
            heights.push((hash, height));
        }

        Ok(heights)
    }

    /// Load all UTXOs from storage.
    pub fn load_all_utxos(&self) -> Result<Vec<(OutPoint, Utxo)>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        let mut utxos = Vec::new();

        let iter = self
            .utxos
            .iter(&rtxn)
            .map_err(|e| StorageError::Read(e.to_string()))?;

        for result in iter {
            let (outpoint, utxo) = result.map_err(|e| StorageError::Read(e.to_string()))?;
            utxos.push((outpoint, utxo));
        }

        Ok(utxos)
    }

    /// Load all transaction index entries from storage.
    pub fn load_all_tx_index(&self) -> Result<Vec<(Hash, Hash)>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        let mut entries = Vec::new();

        let iter = self
            .tx_index
            .iter(&rtxn)
            .map_err(|e| StorageError::Read(e.to_string()))?;

        for result in iter {
            let (txid, block_hash) = result.map_err(|e| StorageError::Read(e.to_string()))?;
            entries.push((txid, block_hash));
        }

        Ok(entries)
    }

    fn get_metadata_u64(&self, key: &str) -> Result<Option<u64>, StorageError> {
        match self.get_metadata(key)? {
            Some(bytes) if bytes.len() == 8 => {
                let mut arr = [0u8; 8];
                arr.copy_from_slice(&bytes);
                Ok(Some(u64::from_le_bytes(arr)))
            }
            Some(bytes) => Err(StorageError::Corruption(format!(
                "invalid u64 length for {}: {}",
                key,
                bytes.len()
            ))),
            None => Ok(None),
        }
    }

    fn get_metadata_hash(&self, key: &str) -> Result<Option<Hash>, StorageError> {
        match self.get_metadata(key)? {
            Some(bytes) if bytes.len() == Hash::SIZE => {
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&bytes);
                Ok(Some(Hash::from_bytes(arr)))
            }
            Some(bytes) => Err(StorageError::Corruption(format!(
                "invalid hash length for {}: {}",
                key,
                bytes.len()
            ))),
            None => Ok(None),
        }
    }
}

impl StorageRead for LmdbStorage {
    fn get_block(&self, hash: &Hash) -> Result<Option<Block>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        self.blocks
            .get(&rtxn, hash)
            .map_err(|e| StorageError::Read(e.to_string()))
    }

    fn get_height(&self, hash: &Hash) -> Result<Option<u64>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        self.heights
            .get(&rtxn, hash)
            .map_err(|e| StorageError::Read(e.to_string()))
    }

    fn get_utxo(&self, outpoint: &OutPoint) -> Result<Option<Utxo>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        self.utxos
            .get(&rtxn, outpoint)
            .map_err(|e| StorageError::Read(e.to_string()))
    }

    fn get_tx_block(&self, txid: &Hash) -> Result<Option<Hash>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        self.tx_index
            .get(&rtxn, txid)
            .map_err(|e| StorageError::Read(e.to_string()))
    }

    fn get_metadata(&self, key: &str) -> Result<Option<Vec<u8>>, StorageError> {
        let rtxn = self
            .env
            .read_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        self.metadata
            .get(&rtxn, key)
            .map(|opt| opt.map(|b| b.to_vec()))
            .map_err(|e| StorageError::Read(e.to_string()))
    }

    fn get_tip_hash(&self) -> Result<Option<Hash>, StorageError> {
        self.get_metadata_hash(metadata_keys::TIP_HASH)
    }

    fn get_tip_height(&self) -> Result<Option<u64>, StorageError> {
        self.get_metadata_u64(metadata_keys::TIP_HEIGHT)
    }

    fn get_genesis_hash(&self) -> Result<Option<Hash>, StorageError> {
        self.get_metadata_hash(metadata_keys::GENESIS_HASH)
    }
}

/// Write transaction for LMDB storage.
pub struct LmdbWriteTxn<'a> {
    txn: heed::RwTxn<'a>,
    blocks: &'a Database<HashCodec, BlockCodec>,
    heights: &'a Database<HashCodec, U64Codec>,
    utxos: &'a Database<OutPointCodec, UtxoCodec>,
    tx_index: &'a Database<HashCodec, HashCodec>,
    metadata: &'a Database<StrCodec, Bytes>,
}

impl<'a> StorageWriteTxn for LmdbWriteTxn<'a> {
    fn put_block(&mut self, hash: &Hash, block: &Block) -> Result<(), StorageError> {
        self.blocks
            .put(&mut self.txn, hash, block)
            .map_err(|e| StorageError::Write(e.to_string()))
    }

    fn put_height(&mut self, hash: &Hash, height: u64) -> Result<(), StorageError> {
        self.heights
            .put(&mut self.txn, hash, &height)
            .map_err(|e| StorageError::Write(e.to_string()))
    }

    fn put_utxo(&mut self, outpoint: &OutPoint, utxo: &Utxo) -> Result<(), StorageError> {
        self.utxos
            .put(&mut self.txn, outpoint, utxo)
            .map_err(|e| StorageError::Write(e.to_string()))
    }

    fn delete_utxo(&mut self, outpoint: &OutPoint) -> Result<bool, StorageError> {
        self.utxos
            .delete(&mut self.txn, outpoint)
            .map_err(|e| StorageError::Delete(e.to_string()))
    }

    fn put_tx_index(&mut self, txid: &Hash, block_hash: &Hash) -> Result<(), StorageError> {
        self.tx_index
            .put(&mut self.txn, txid, block_hash)
            .map_err(|e| StorageError::Write(e.to_string()))
    }

    fn delete_tx_index(&mut self, txid: &Hash) -> Result<bool, StorageError> {
        self.tx_index
            .delete(&mut self.txn, txid)
            .map_err(|e| StorageError::Delete(e.to_string()))
    }

    fn set_tip(&mut self, hash: &Hash, height: u64) -> Result<(), StorageError> {
        self.metadata
            .put(&mut self.txn, metadata_keys::TIP_HASH, hash.as_bytes())
            .map_err(|e| StorageError::Write(e.to_string()))?;
        self.metadata
            .put(
                &mut self.txn,
                metadata_keys::TIP_HEIGHT,
                &height.to_le_bytes(),
            )
            .map_err(|e| StorageError::Write(e.to_string()))
    }

    fn put_metadata(&mut self, key: &str, value: &[u8]) -> Result<(), StorageError> {
        self.metadata
            .put(&mut self.txn, key, value)
            .map_err(|e| StorageError::Write(e.to_string()))
    }

    fn commit(self) -> Result<(), StorageError> {
        self.txn
            .commit()
            .map_err(|e| StorageError::Commit(e.to_string()))
    }

    fn abort(self) {
        self.txn.abort();
    }
}

impl StorageWrite for LmdbStorage {
    type WriteTxn<'a> = LmdbWriteTxn<'a>;

    fn write_txn(&self) -> Result<Self::WriteTxn<'_>, StorageError> {
        let txn = self
            .env
            .write_txn()
            .map_err(|e| StorageError::Transaction(e.to_string()))?;
        Ok(LmdbWriteTxn {
            txn,
            blocks: &self.blocks,
            heights: &self.heights,
            utxos: &self.utxos,
            tx_index: &self.tx_index,
            metadata: &self.metadata,
        })
    }

    fn init_genesis(
        &self,
        genesis: &Block,
        difficulty_interval: u64,
        target_block_time: u64,
        initial_reward: u64,
        halving_interval: u64,
    ) -> Result<(), StorageError> {
        let genesis_hash = genesis.hash();

        // Check if already initialized
        if let Some(existing_genesis) = self.get_genesis_hash()? {
            if existing_genesis != genesis_hash {
                return Err(StorageError::GenesisMismatch {
                    expected: genesis_hash.to_hex(),
                    found: existing_genesis.to_hex(),
                });
            }
            // Already initialized with same genesis, nothing to do
            return Ok(());
        }

        let mut wtxn = self.write_txn()?;

        // Store the genesis block
        wtxn.put_block(&genesis_hash, genesis)?;
        wtxn.put_height(&genesis_hash, 0)?;

        // Add genesis block UTXOs and index transactions
        for (i, tx) in genesis.transactions.iter().enumerate() {
            let txid = tx.txid();
            wtxn.put_tx_index(&txid, &genesis_hash)?;
            for (j, output) in tx.outputs.iter().enumerate() {
                let outpoint = OutPoint::new(txid, j as u32);
                wtxn.put_utxo(
                    &outpoint,
                    &Utxo {
                        output: output.clone(),
                        height: 0,
                        is_coinbase: i == 0,
                    },
                )?;
            }
        }

        // Set chain tip
        wtxn.set_tip(&genesis_hash, 0)?;

        // Store chain parameters
        wtxn.put_metadata(metadata_keys::GENESIS_HASH, genesis_hash.as_bytes())?;
        wtxn.put_metadata(
            metadata_keys::DIFFICULTY_INTERVAL,
            &difficulty_interval.to_le_bytes(),
        )?;
        wtxn.put_metadata(
            metadata_keys::TARGET_BLOCK_TIME,
            &target_block_time.to_le_bytes(),
        )?;
        wtxn.put_metadata(metadata_keys::INITIAL_REWARD, &initial_reward.to_le_bytes())?;
        wtxn.put_metadata(
            metadata_keys::HALVING_INTERVAL,
            &halving_interval.to_le_bytes(),
        )?;

        wtxn.commit()
    }
}
