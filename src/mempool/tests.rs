//! Unit tests for the mempool module.

use crate::blockchain::{
    Address, Block, BlockHeader, Blockchain, OutPoint, Transaction, TxInput, TxOutput, Witness,
    create_genesis_block,
};
use crate::constants::TX_EXPIRY_BLOCKS;
use crate::crypto::{hash, ml_dsa_87};
use crate::mempool::{Mempool, MempoolError};

// Helper to create a test blockchain with mature coinbase
fn test_blockchain() -> (
    Blockchain,
    crate::crypto::PublicKey,
    crate::crypto::SecretKey,
    Address,
) {
    let (pk, sk) = ml_dsa_87::keygen();
    let address = Address::from_public_key(&pk);

    let mut timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - (crate::constants::COINBASE_MATURITY + 10) * 600;

    let genesis = create_genesis_block(timestamp, 0x40ffffff, 50_000_000, address);
    let mut chain = Blockchain::new(genesis, 2016, 600, 50_000_000, 210_000);

    // Add COINBASE_MATURITY blocks to mature the genesis coinbase
    for _ in 0..crate::constants::COINBASE_MATURITY {
        timestamp += 600;
        let prev_block = chain.tip();
        let height = chain.height() + 1;
        let reward = chain.block_reward(height);

        let coinbase = Transaction::coinbase(height, reward, address);
        let merkle_root = Block::compute_merkle_root(std::slice::from_ref(&coinbase));

        let header = BlockHeader {
            version: BlockHeader::CURRENT_VERSION,
            prev_hash: prev_block.hash(),
            merkle_root,
            timestamp,
            difficulty_bits: prev_block.header.difficulty_bits,
            nonce: [0u8; 32],
        };

        let block = Block::new(header, vec![coinbase]);
        chain.add_block(block).unwrap();
    }

    (chain, pk, sk, address)
}

// Helper to create a valid spending transaction with sufficient fee
fn create_spending_tx(
    chain: &Blockchain,
    pk: &crate::crypto::PublicKey,
    sk: &crate::crypto::SecretKey,
    address: Address,
) -> Transaction {
    create_spending_tx_with_fee(chain, pk, sk, address, 20_000_000)
}

// Helper to create a spending transaction with a specific fee
fn create_spending_tx_with_fee(
    chain: &Blockchain,
    pk: &crate::crypto::PublicKey,
    sk: &crate::crypto::SecretKey,
    address: Address,
    fee: u64,
) -> Transaction {
    let utxos = chain.utxos_for_address(&address);
    let (outpoint, utxo) = utxos
        .into_iter()
        .find(|(_, u)| {
            !u.is_coinbase || chain.height() >= u.height + crate::constants::COINBASE_MATURITY
        })
        .expect("no spendable UTXO");

    let mut tx = Transaction::new(
        vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: Box::new(pk.clone()),
                signature: Box::new(ml_dsa_87::sign(sk, &[0u8; 64])), // placeholder
            },
        )],
        vec![TxOutput::p2pkh(utxo.output.amount - fee, address)],
    );

    // Sign properly
    let signing_data = tx.signing_data(0);
    let message_hash = hash(&signing_data);
    let signature = ml_dsa_87::sign(sk, message_hash.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };

    tx
}

#[test]
fn test_mempool_add_valid_transaction() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    let tx = create_spending_tx(&chain, &pk, &sk, address);
    let result = mempool.add(tx, &chain, current_height);

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(mempool.len(), 1);
}

#[test]
fn test_mempool_double_add_same_tx() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    let tx = create_spending_tx(&chain, &pk, &sk, address);
    let txid = tx.txid();

    // Add first time
    assert!(mempool.add(tx.clone(), &chain, current_height).unwrap());
    // Add second time - should return false (already present)
    assert!(!mempool.add(tx, &chain, current_height).unwrap());
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&txid));
}

#[test]
fn test_mempool_double_spend_detection() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    // Find a spendable UTXO that both transactions will use
    let utxos = chain.utxos_for_address(&address);
    let (outpoint, utxo) = utxos
        .into_iter()
        .find(|(_, u)| {
            !u.is_coinbase || chain.height() >= u.height + crate::constants::COINBASE_MATURITY
        })
        .expect("no spendable UTXO");

    // Create first transaction spending the UTXO
    let mut tx1 = Transaction::new(
        vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: Box::new(pk.clone()),
                signature: Box::new(ml_dsa_87::sign(&sk, &[0u8; 64])),
            },
        )],
        vec![TxOutput::p2pkh(utxo.output.amount - 20_000_000, address)],
    );
    let signing_data = tx1.signing_data(0);
    let message_hash = hash(&signing_data);
    let signature = ml_dsa_87::sign(&sk, message_hash.as_bytes());
    tx1.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };
    let tx1_id = tx1.txid();

    // Create second transaction spending the same UTXO (different output amount)
    let mut tx2 = Transaction::new(
        vec![TxInput::new(
            outpoint, // Same outpoint as tx1!
            Witness::P2PKH {
                public_key: Box::new(pk.clone()),
                signature: Box::new(ml_dsa_87::sign(&sk, &[0u8; 64])),
            },
        )],
        vec![TxOutput::p2pkh(utxo.output.amount - 25_000_000, address)], // different amount
    );
    let signing_data = tx2.signing_data(0);
    let message_hash = hash(&signing_data);
    let signature = ml_dsa_87::sign(&sk, message_hash.as_bytes());
    tx2.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };

    // Add first transaction
    assert!(mempool.add(tx1, &chain, current_height).is_ok());

    // Try to add second (conflicting) transaction - should fail with DoubleSpend
    let result = mempool.add(tx2, &chain, current_height);
    assert!(
        matches!(result, Err(MempoolError::DoubleSpend(id)) if id == tx1_id),
        "Expected DoubleSpend error with tx1_id, got: {result:?}"
    );
}

#[test]
fn test_mempool_remove_confirmed() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    let tx = create_spending_tx(&chain, &pk, &sk, address);
    let txid = tx.txid();

    // Add to mempool
    assert!(mempool.add(tx.clone(), &chain, current_height).unwrap());
    assert_eq!(mempool.len(), 1);

    // Simulate block confirmation
    mempool.remove_confirmed(&[tx]);
    assert_eq!(mempool.len(), 0);
    assert!(!mempool.contains(&txid));
}

#[test]
fn test_mempool_remove_conflicts() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    let tx = create_spending_tx(&chain, &pk, &sk, address);
    let spent_outpoint = tx.inputs[0].outpoint;

    // Add to mempool
    assert!(mempool.add(tx.clone(), &chain, current_height).unwrap());
    assert_eq!(mempool.len(), 1);

    // Create a "block" transaction that spends the same UTXO
    let block_tx = Transaction::new(
        vec![TxInput::new(
            spent_outpoint,
            Witness::Coinbase(vec![]), // Fake witness, doesn't matter
        )],
        vec![TxOutput::p2pkh(1000, address)],
    );

    // Remove conflicts
    mempool.remove_conflicts(&[block_tx]);
    assert_eq!(mempool.len(), 0);
}

#[test]
fn test_mempool_get_block_txs_respects_limit() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    // Add a transaction
    let tx = create_spending_tx(&chain, &pk, &sk, address);
    mempool.add(tx, &chain, current_height).unwrap();

    // Get with limit 0
    let txs = mempool.get_block_txs(0);
    assert_eq!(txs.len(), 0);

    // Get with limit 1
    let txs = mempool.get_block_txs(1);
    assert_eq!(txs.len(), 1);

    // Get with limit larger than mempool
    let txs = mempool.get_block_txs(100);
    assert_eq!(txs.len(), 1);
}

#[test]
fn test_mempool_rejects_coinbase() {
    let (chain, _pk, _sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    let coinbase = Transaction::coinbase(100, 50_000_000, address);
    let result = mempool.add(coinbase, &chain, current_height);

    assert!(matches!(result, Err(MempoolError::CoinbaseNotAllowed)));
}

#[test]
fn test_mempool_rejects_missing_input() {
    let (chain, pk, sk, _address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    // Create transaction referencing non-existent UTXO
    let fake_outpoint = OutPoint::new(hash(b"fake tx"), 0);
    let (other_pk, _) = ml_dsa_87::keygen();
    let other_address = Address::from_public_key(&other_pk);

    let mut tx = Transaction::new(
        vec![TxInput::new(
            fake_outpoint,
            Witness::P2PKH {
                public_key: Box::new(pk.clone()),
                signature: Box::new(ml_dsa_87::sign(&sk, &[0u8; 64])),
            },
        )],
        vec![TxOutput::p2pkh(1000, other_address)],
    );

    let signing_data = tx.signing_data(0);
    let message_hash = hash(&signing_data);
    let signature = ml_dsa_87::sign(&sk, message_hash.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };

    let result = mempool.add(tx, &chain, current_height);
    assert!(matches!(result, Err(MempoolError::MissingInput(_))));
}

#[test]
fn test_mempool_txids() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    let tx = create_spending_tx(&chain, &pk, &sk, address);
    let txid = tx.txid();

    mempool.add(tx, &chain, current_height).unwrap();

    let txids = mempool.txids();
    assert_eq!(txids.len(), 1);
    assert!(txids.contains(&txid));
}

#[test]
fn test_chained_transactions_not_supported() {
    // This test documents that chained unconfirmed transactions are NOT supported.
    // A transaction cannot spend an output created by another mempool transaction.
    // This is a deliberate simplification - see module documentation.

    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    // Create first transaction (TX1) that creates an output
    let tx1 = create_spending_tx(&chain, &pk, &sk, address);
    let tx1_txid = tx1.txid();
    let tx1_output_amount = tx1.outputs[0].amount;

    // Add TX1 to mempool
    assert!(mempool.add(tx1, &chain, current_height).unwrap());
    assert_eq!(mempool.len(), 1);

    // Try to create TX2 that spends TX1's output (which is only in mempool, not blockchain)
    let tx2_outpoint = OutPoint::new(tx1_txid, 0);
    let mut tx2 = Transaction::new(
        vec![TxInput::new(
            tx2_outpoint,
            Witness::P2PKH {
                public_key: Box::new(pk.clone()),
                signature: Box::new(ml_dsa_87::sign(&sk, &[0u8; 64])), // placeholder
            },
        )],
        vec![TxOutput::p2pkh(tx1_output_amount - 20_000_000, address)],
    );

    // Sign TX2
    let signing_data = tx2.signing_data(0);
    let message_hash = hash(&signing_data);
    let signature = ml_dsa_87::sign(&sk, message_hash.as_bytes());
    tx2.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };

    // TX2 should be rejected because TX1's output is not in the blockchain UTXO set
    let result = mempool.add(tx2, &chain, current_height);
    assert!(
        matches!(result, Err(MempoolError::MissingInput(op)) if op == tx2_outpoint),
        "Chained transactions should be rejected with MissingInput error"
    );
}

#[test]
fn test_mempool_rejects_low_fee() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();
    let current_height = chain.height();

    // Create a transaction with very low fee (1 satoshi)
    let tx = create_spending_tx_with_fee(&chain, &pk, &sk, address, 1);
    let result = mempool.add(tx, &chain, current_height);

    assert!(matches!(result, Err(MempoolError::FeeTooLow { .. })));
}

#[test]
fn test_mempool_cleanup_expired() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();

    // Add transaction at height 100
    let tx = create_spending_tx(&chain, &pk, &sk, address);
    let txid = tx.txid();
    mempool.add(tx, &chain, 100).unwrap();
    assert_eq!(mempool.len(), 1);

    // Cleanup at height 100 + TX_EXPIRY_BLOCKS - 1 should NOT remove the tx
    mempool.cleanup_expired(100 + TX_EXPIRY_BLOCKS - 1);
    assert_eq!(mempool.len(), 1);
    assert!(mempool.contains(&txid));

    // Cleanup at height 100 + TX_EXPIRY_BLOCKS should remove the tx
    mempool.cleanup_expired(100 + TX_EXPIRY_BLOCKS);
    assert_eq!(mempool.len(), 0);
    assert!(!mempool.contains(&txid));
}

#[test]
fn test_mempool_entry_tracks_added_height() {
    let (chain, pk, sk, address) = test_blockchain();
    let mut mempool = Mempool::new();

    let tx = create_spending_tx(&chain, &pk, &sk, address);
    let txid = tx.txid();
    let added_height = 12345;

    mempool.add(tx, &chain, added_height).unwrap();

    let entry = mempool.get_entry(&txid).unwrap();
    assert_eq!(entry.added_height, added_height);
}
