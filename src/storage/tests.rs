//! Storage layer tests.

use tempfile::TempDir;

use super::codecs::{BlockCodec, HashCodec, OutPointCodec, U64Codec, UtxoCodec};
use super::lmdb::LmdbStorage;
use super::{StorageRead, StorageWrite, StorageWriteTxn};
use crate::blockchain::{
    Address, Block, LockingCondition, OutPoint, TxOutput, Utxo, create_genesis_block,
};
use crate::crypto::{Hash, hash};
use heed::{BytesDecode, BytesEncode};

// ============================================================================
// Codec Tests
// ============================================================================

#[test]
fn test_hash_codec_roundtrip() {
    let h = hash(b"test data for hashing");
    let encoded = HashCodec::bytes_encode(&h).expect("encode failed");
    let decoded = HashCodec::bytes_decode(&encoded).expect("decode failed");
    assert_eq!(h, decoded);
}

#[test]
fn test_hash_codec_zero() {
    let h = Hash::from_bytes([0u8; 64]);
    let encoded = HashCodec::bytes_encode(&h).expect("encode failed");
    let decoded = HashCodec::bytes_decode(&encoded).expect("decode failed");
    assert_eq!(h, decoded);
}

#[test]
fn test_outpoint_codec_roundtrip() {
    let txid = hash(b"transaction");
    let outpoint = OutPoint::new(txid, 42);
    let encoded = OutPointCodec::bytes_encode(&outpoint).expect("encode failed");
    let decoded = OutPointCodec::bytes_decode(&encoded).expect("decode failed");
    assert_eq!(outpoint, decoded);
}

#[test]
fn test_outpoint_codec_null() {
    let outpoint = OutPoint::null();
    let encoded = OutPointCodec::bytes_encode(&outpoint).expect("encode failed");
    let decoded = OutPointCodec::bytes_decode(&encoded).expect("decode failed");
    assert_eq!(outpoint, decoded);
    assert!(decoded.is_null());
}

#[test]
fn test_u64_codec_roundtrip() {
    for value in [0u64, 1, 100, 1_000_000, u64::MAX / 2, u64::MAX] {
        let encoded = U64Codec::bytes_encode(&value).expect("encode failed");
        let decoded = U64Codec::bytes_decode(&encoded).expect("decode failed");
        assert_eq!(value, decoded, "roundtrip failed for {value}");
    }
}

#[test]
fn test_u64_codec_ordering() {
    // Big-endian encoding should preserve numeric ordering
    let values = [0u64, 1, 100, 1000, u64::MAX];
    let mut encoded: Vec<_> = values
        .iter()
        .map(|v| U64Codec::bytes_encode(v).unwrap().to_vec())
        .collect();
    encoded.sort();
    let decoded: Vec<_> = encoded
        .iter()
        .map(|e| U64Codec::bytes_decode(e).unwrap())
        .collect();
    assert_eq!(decoded, values.to_vec());
}

#[test]
fn test_utxo_codec_roundtrip() {
    let address = Address::from_hash(hash(b"test address"));
    let utxo = Utxo {
        output: TxOutput {
            amount: 50_000_000,
            condition: LockingCondition::P2PKH(address),
        },
        height: 100,
        is_coinbase: true,
    };
    let encoded = UtxoCodec::bytes_encode(&utxo).expect("encode failed");
    let decoded = UtxoCodec::bytes_decode(&encoded).expect("decode failed");
    assert_eq!(utxo, decoded);
}

#[test]
fn test_utxo_codec_non_coinbase() {
    let address = Address::from_hash(hash(b"another address"));
    let utxo = Utxo {
        output: TxOutput {
            amount: 1_000_000,
            condition: LockingCondition::P2PKH(address),
        },
        height: 500,
        is_coinbase: false,
    };
    let encoded = UtxoCodec::bytes_encode(&utxo).expect("encode failed");
    let decoded = UtxoCodec::bytes_decode(&encoded).expect("decode failed");
    assert_eq!(utxo, decoded);
}

#[test]
fn test_block_codec_roundtrip() {
    let address = Address::from_hash(hash(b"miner"));
    let genesis = create_genesis_block(0, 0x20ffffff, 50_000_000, address);
    let encoded = BlockCodec::bytes_encode(&genesis).expect("encode failed");
    let decoded = BlockCodec::bytes_decode(&encoded).expect("decode failed");
    assert_eq!(genesis, decoded);
}

// ============================================================================
// LMDB Storage Tests
// ============================================================================

fn create_test_storage() -> (TempDir, LmdbStorage) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let storage = LmdbStorage::open(dir.path()).expect("failed to open storage");
    (dir, storage)
}

fn create_test_genesis() -> Block {
    let address = Address::from_hash(hash(b"test genesis miner"));
    create_genesis_block(0, 0x20ffffff, 50_000_000, address)
}

#[test]
fn test_storage_open() {
    let (_dir, storage) = create_test_storage();
    assert!(!storage.is_initialized().unwrap());
}

#[test]
fn test_storage_init_genesis() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("init_genesis failed");

    assert!(storage.is_initialized().unwrap());

    // Verify genesis was stored
    let genesis_hash = genesis.hash();
    let stored_genesis = storage.get_genesis_hash().unwrap().unwrap();
    assert_eq!(genesis_hash, stored_genesis);

    // Verify block was stored
    let stored_block = storage.get_block(&genesis_hash).unwrap().unwrap();
    assert_eq!(genesis, stored_block);

    // Verify height was stored
    let stored_height = storage.get_height(&genesis_hash).unwrap().unwrap();
    assert_eq!(0, stored_height);

    // Verify tip was set
    let tip_hash = storage.get_tip_hash().unwrap().unwrap();
    assert_eq!(genesis_hash, tip_hash);
    let tip_height = storage.get_tip_height().unwrap().unwrap();
    assert_eq!(0, tip_height);
}

#[test]
fn test_storage_init_genesis_twice() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("first init_genesis failed");

    // Second init with same genesis should be OK
    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("second init_genesis with same genesis failed");
}

#[test]
fn test_storage_genesis_mismatch() {
    let (_dir, storage) = create_test_storage();
    let genesis1 = create_test_genesis();
    let genesis2 = {
        let address = Address::from_hash(hash(b"different miner"));
        create_genesis_block(1, 0x20ffffff, 50_000_000, address)
    };

    storage
        .init_genesis(&genesis1, 2016, 600, 50_000_000, 210_000)
        .expect("first init_genesis failed");

    // Second init with different genesis should fail
    let result = storage.init_genesis(&genesis2, 2016, 600, 50_000_000, 210_000);
    assert!(matches!(
        result,
        Err(super::StorageError::GenesisMismatch { .. })
    ));
}

#[test]
fn test_storage_block_operations() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();
    let genesis_hash = genesis.hash();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("init_genesis failed");

    // Write a new "block" (just another genesis for simplicity)
    let address = Address::from_hash(hash(b"block 1 miner"));
    let block1 = create_genesis_block(100, 0x20ffffff, 50_000_000, address);
    let block1_hash = block1.hash();

    let mut wtxn = storage.write_txn().expect("write_txn failed");
    wtxn.put_block(&block1_hash, &block1)
        .expect("put_block failed");
    wtxn.put_height(&block1_hash, 1).expect("put_height failed");
    wtxn.set_tip(&block1_hash, 1).expect("set_tip failed");
    wtxn.commit().expect("commit failed");

    // Verify block was stored
    let stored_block = storage.get_block(&block1_hash).unwrap().unwrap();
    assert_eq!(block1, stored_block);

    // Verify height
    let stored_height = storage.get_height(&block1_hash).unwrap().unwrap();
    assert_eq!(1, stored_height);

    // Verify tip was updated
    let tip_hash = storage.get_tip_hash().unwrap().unwrap();
    assert_eq!(block1_hash, tip_hash);
    let tip_height = storage.get_tip_height().unwrap().unwrap();
    assert_eq!(1, tip_height);

    // Verify genesis is still there
    let stored_genesis = storage.get_block(&genesis_hash).unwrap().unwrap();
    assert_eq!(genesis, stored_genesis);
}

#[test]
fn test_storage_utxo_operations() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("init_genesis failed");

    // Create a test UTXO
    let txid = hash(b"test transaction");
    let outpoint = OutPoint::new(txid, 0);
    let address = Address::from_hash(hash(b"recipient"));
    let utxo = Utxo {
        output: TxOutput {
            amount: 1_000_000,
            condition: LockingCondition::P2PKH(address),
        },
        height: 5,
        is_coinbase: false,
    };

    // Add UTXO
    let mut wtxn = storage.write_txn().expect("write_txn failed");
    wtxn.put_utxo(&outpoint, &utxo).expect("put_utxo failed");
    wtxn.commit().expect("commit failed");

    // Verify UTXO was stored
    let stored_utxo = storage.get_utxo(&outpoint).unwrap().unwrap();
    assert_eq!(utxo, stored_utxo);

    // Delete UTXO
    let mut wtxn = storage.write_txn().expect("write_txn failed");
    let deleted = wtxn.delete_utxo(&outpoint).expect("delete_utxo failed");
    assert!(deleted);
    wtxn.commit().expect("commit failed");

    // Verify UTXO was deleted
    let stored_utxo = storage.get_utxo(&outpoint).unwrap();
    assert!(stored_utxo.is_none());
}

#[test]
fn test_storage_tx_index_operations() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();
    let genesis_hash = genesis.hash();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("init_genesis failed");

    // Genesis transactions should be indexed
    let coinbase_txid = genesis.transactions[0].txid();
    let block_hash = storage.get_tx_block(&coinbase_txid).unwrap().unwrap();
    assert_eq!(genesis_hash, block_hash);

    // Add a new tx index entry
    let new_txid = hash(b"new transaction");
    let new_block_hash = hash(b"block containing tx");

    let mut wtxn = storage.write_txn().expect("write_txn failed");
    wtxn.put_tx_index(&new_txid, &new_block_hash)
        .expect("put_tx_index failed");
    wtxn.commit().expect("commit failed");

    // Verify tx index entry
    let stored_block_hash = storage.get_tx_block(&new_txid).unwrap().unwrap();
    assert_eq!(new_block_hash, stored_block_hash);

    // Delete tx index entry
    let mut wtxn = storage.write_txn().expect("write_txn failed");
    let deleted = wtxn
        .delete_tx_index(&new_txid)
        .expect("delete_tx_index failed");
    assert!(deleted);
    wtxn.commit().expect("commit failed");

    // Verify tx index entry was deleted
    let stored_block_hash = storage.get_tx_block(&new_txid).unwrap();
    assert!(stored_block_hash.is_none());
}

#[test]
fn test_storage_transaction_abort() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("init_genesis failed");

    let txid = hash(b"test transaction");
    let outpoint = OutPoint::new(txid, 0);
    let address = Address::from_hash(hash(b"recipient"));
    let utxo = Utxo {
        output: TxOutput {
            amount: 1_000_000,
            condition: LockingCondition::P2PKH(address),
        },
        height: 5,
        is_coinbase: false,
    };

    // Add UTXO but abort
    let mut wtxn = storage.write_txn().expect("write_txn failed");
    wtxn.put_utxo(&outpoint, &utxo).expect("put_utxo failed");
    wtxn.abort();

    // Verify UTXO was NOT stored
    let stored_utxo = storage.get_utxo(&outpoint).unwrap();
    assert!(stored_utxo.is_none());
}

#[test]
fn test_storage_load_all() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("init_genesis failed");

    // Load all blocks
    let blocks = storage.load_all_blocks().unwrap();
    assert_eq!(1, blocks.len());
    assert_eq!(genesis, blocks[0].1);

    // Load all heights
    let heights = storage.load_all_heights().unwrap();
    assert_eq!(1, heights.len());
    assert_eq!(0, heights[0].1);

    // Load all UTXOs (genesis coinbase output)
    let utxos = storage.load_all_utxos().unwrap();
    assert!(!utxos.is_empty());

    // Load all tx index entries
    let tx_index = storage.load_all_tx_index().unwrap();
    assert!(!tx_index.is_empty());
}

#[test]
fn test_storage_load_config() {
    let (_dir, storage) = create_test_storage();
    let genesis = create_test_genesis();
    let genesis_hash = genesis.hash();

    storage
        .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
        .expect("init_genesis failed");

    let config = storage.load_config().unwrap().unwrap();
    assert_eq!(genesis_hash, config.genesis_hash);
    assert_eq!(genesis_hash, config.tip_hash);
    assert_eq!(0, config.tip_height);
    assert_eq!(2016, config.difficulty_interval);
    assert_eq!(600, config.target_block_time);
    assert_eq!(50_000_000, config.initial_reward);
    assert_eq!(210_000, config.halving_interval);
}

#[test]
fn test_storage_persistence() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let genesis = create_test_genesis();
    let genesis_hash = genesis.hash();

    // Create and initialize storage
    {
        let storage = LmdbStorage::open(dir.path()).expect("failed to open storage");
        storage
            .init_genesis(&genesis, 2016, 600, 50_000_000, 210_000)
            .expect("init_genesis failed");
    }

    // Reopen storage and verify data persisted
    {
        let storage = LmdbStorage::open(dir.path()).expect("failed to reopen storage");
        assert!(storage.is_initialized().unwrap());

        let stored_genesis = storage.get_genesis_hash().unwrap().unwrap();
        assert_eq!(genesis_hash, stored_genesis);

        let config = storage.load_config().unwrap().unwrap();
        assert_eq!(genesis_hash, config.genesis_hash);
    }
}
