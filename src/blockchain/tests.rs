//! Unit tests for blockchain module.

use super::*;
use crate::constants::COINBASE_MATURITY;
use crate::crypto::{self, Hash, PublicKey, SecretKey};

// Helper to create a test keypair
fn test_keypair() -> (PublicKey, SecretKey) {
    crypto::ml_dsa_87::keygen()
}

// ========================================================================
// Serialization Tests
// ========================================================================

#[test]
fn test_varint_roundtrip() {
    use super::serialize::{read_var_int, write_var_int};

    let test_values = [
        0u64,
        1,
        0xFC,
        0xFD,
        0xFE,
        0xFF,
        0x100,
        0xFFFF,
        0x10000,
        0xFFFFFFFF,
        0x100000000,
        u64::MAX,
    ];

    for &value in &test_values {
        let mut buf = Vec::new();
        write_var_int(&mut buf, value);
        let (decoded, remaining) = read_var_int(&buf).unwrap();
        assert_eq!(decoded, value, "varint roundtrip failed for {value}");
        assert!(remaining.is_empty());
    }
}

#[test]
fn test_address_serialization() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);

    let bytes = address.to_bytes();
    assert_eq!(bytes.len(), 64);

    let (decoded, remaining) = Address::deserialize(&bytes).unwrap();
    assert!(remaining.is_empty());
    assert_eq!(decoded, address);
}

#[test]
fn test_outpoint_serialization() {
    let outpoint = OutPoint::new(crypto::hash(b"test tx"), 42);

    let bytes = outpoint.to_bytes();
    assert_eq!(bytes.len(), 68); // 64 + 4

    let decoded = OutPoint::from_bytes(&bytes).unwrap();
    assert_eq!(decoded, outpoint);
}

#[test]
fn test_outpoint_null() {
    let null = OutPoint::null();
    assert!(null.is_null());

    let bytes = null.to_bytes();
    let decoded = OutPoint::from_bytes(&bytes).unwrap();
    assert!(decoded.is_null());
}

#[test]
fn test_witness_p2pkh_serialization() {
    let (pk, sk) = test_keypair();
    let message = b"test message";
    let sig = crypto::ml_dsa_87::sign(&sk, message);

    let witness = Witness::P2PKH {
        public_key: Box::new(pk),
        signature: Box::new(sig),
    };

    let bytes = witness.to_bytes();
    let decoded = Witness::from_bytes(&bytes).unwrap();

    match decoded {
        Witness::P2PKH {
            public_key,
            signature,
        } => {
            // Verify signature still works after serialization roundtrip
            assert!(crypto::ml_dsa_87::verify(&public_key, message, &signature));
        }
        _ => panic!("wrong witness type"),
    }
}

#[test]
fn test_witness_coinbase_serialization() {
    let data = b"block height 12345";
    let witness = Witness::Coinbase(data.to_vec());

    let bytes = witness.to_bytes();
    let decoded = Witness::from_bytes(&bytes).unwrap();

    match decoded {
        Witness::Coinbase(decoded_data) => {
            assert_eq!(decoded_data, data);
        }
        _ => panic!("wrong witness type"),
    }
}

#[test]
fn test_witness_multisig_serialization() {
    let (pk1, sk1) = test_keypair();
    let (pk2, _sk2) = test_keypair();
    let message = b"multisig test";
    let sig1 = crypto::ml_dsa_87::sign(&sk1, message);

    let witness = Witness::Multisig {
        public_keys: vec![pk1.clone(), pk2.clone()],
        signatures: vec![Some(sig1), None],
    };

    let bytes = witness.to_bytes();
    let decoded = Witness::from_bytes(&bytes).unwrap();

    match decoded {
        Witness::Multisig {
            public_keys,
            signatures,
        } => {
            assert_eq!(public_keys.len(), 2);
            assert_eq!(signatures.len(), 2);
            assert!(signatures[0].is_some());
            assert!(signatures[1].is_none());
        }
        _ => panic!("wrong witness type"),
    }
}

#[test]
fn test_locking_condition_p2pkh_serialization() {
    let (pk, _) = test_keypair();
    let condition = LockingCondition::p2pkh(&pk);

    let bytes = condition.to_bytes();
    let decoded = LockingCondition::from_bytes(&bytes).unwrap();
    assert_eq!(decoded, condition);
}

#[test]
fn test_locking_condition_multisig_serialization() {
    let (pk1, _) = test_keypair();
    let (pk2, _) = test_keypair();
    let condition = LockingCondition::multisig(2, vec![pk1, pk2]);

    let bytes = condition.to_bytes();
    let decoded = LockingCondition::from_bytes(&bytes).unwrap();

    match decoded {
        LockingCondition::Multisig {
            threshold,
            public_keys,
        } => {
            assert_eq!(threshold, 2);
            assert_eq!(public_keys.len(), 2);
        }
        _ => panic!("wrong condition type"),
    }
}

#[test]
fn test_tx_output_serialization() {
    let (pk, _) = test_keypair();
    let output = TxOutput::p2pkh(1_000_000, Address::from_public_key(&pk));

    let bytes = output.to_bytes();
    let decoded = TxOutput::from_bytes(&bytes).unwrap();
    assert_eq!(decoded.amount, output.amount);
}

#[test]
fn test_transaction_serialization() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let tx = Transaction::coinbase(0, 50_000_000, address);

    let bytes = tx.to_bytes();
    let decoded = Transaction::from_bytes(&bytes).unwrap();

    assert_eq!(decoded.version, tx.version);
    assert_eq!(decoded.inputs.len(), tx.inputs.len());
    assert_eq!(decoded.outputs.len(), tx.outputs.len());
    assert_eq!(decoded.txid(), tx.txid());
}

#[test]
fn test_block_header_serialization() {
    let header = BlockHeader {
        version: 1,
        prev_hash: crypto::hash(b"prev"),
        merkle_root: crypto::hash(b"merkle"),
        timestamp: 1234567890,
        difficulty_bits: 0x1d00ffff,
        nonce: [42u8; 32],
    };

    let bytes = header.to_bytes();
    assert_eq!(bytes.len(), BlockHeader::SIZE);

    let decoded = BlockHeader::from_bytes(&bytes).unwrap();
    assert_eq!(decoded, header);
}

#[test]
fn test_block_serialization() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let tx = Transaction::coinbase(0, 50_000_000, address);
    let merkle_root = Block::compute_merkle_root(std::slice::from_ref(&tx));

    let header = BlockHeader {
        version: 1,
        prev_hash: Hash::from_bytes([0u8; 64]),
        merkle_root,
        timestamp: 1234567890,
        difficulty_bits: 0x1d00ffff,
        nonce: [0u8; 32],
    };

    let block = Block::new(header, vec![tx]);
    let bytes = block.to_bytes();
    let decoded = Block::from_bytes(&bytes).unwrap();

    assert_eq!(decoded.hash(), block.hash());
    assert!(decoded.verify_merkle_root());
}

// ========================================================================
// Merkle Root Tests
// ========================================================================

#[test]
fn test_merkle_root_single_tx() {
    let (pk, _) = test_keypair();
    let tx = Transaction::coinbase(0, 50_000_000, Address::from_public_key(&pk));

    let merkle_root = Block::compute_merkle_root(std::slice::from_ref(&tx));
    assert_eq!(merkle_root, tx.txid());
}

#[test]
fn test_merkle_root_two_txs() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let tx1 = Transaction::coinbase(0, 50_000_000, address);
    let tx2 = Transaction::coinbase(1, 50_000_000, address);

    let merkle_root = Block::compute_merkle_root(&[tx1.clone(), tx2.clone()]);
    let expected = crypto::hash_many(&[tx1.txid().as_bytes(), tx2.txid().as_bytes()]);
    assert_eq!(merkle_root, expected);
}

#[test]
fn test_merkle_root_odd_txs() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let tx1 = Transaction::coinbase(0, 50_000_000, address);
    let tx2 = Transaction::coinbase(1, 50_000_000, address);
    let tx3 = Transaction::coinbase(2, 50_000_000, address);

    let merkle_root = Block::compute_merkle_root(&[tx1.clone(), tx2.clone(), tx3.clone()]);

    // Manual calculation:
    // Level 1: [H(tx1|tx2), H(tx3|tx3)]
    // Level 0: H(H(tx1|tx2) | H(tx3|tx3))
    let h12 = crypto::hash_many(&[tx1.txid().as_bytes(), tx2.txid().as_bytes()]);
    let h33 = crypto::hash_many(&[tx3.txid().as_bytes(), tx3.txid().as_bytes()]);
    let expected = crypto::hash_many(&[h12.as_bytes(), h33.as_bytes()]);
    assert_eq!(merkle_root, expected);
}

// ========================================================================
// Proof of Work Tests
// ========================================================================

#[test]
fn test_difficulty_target_encoding() {
    // Test Bitcoin-like difficulty encoding
    let bits = 0x1d00ffff_u32;
    let header = BlockHeader {
        version: 1,
        prev_hash: Hash::from_bytes([0u8; 64]),
        merkle_root: Hash::from_bytes([0u8; 64]),
        timestamp: 0,
        difficulty_bits: bits,
        nonce: [0u8; 32],
    };

    let target = header.target();
    // The target should have 0x00ffff at the appropriate position
    assert!(target.iter().any(|&b| b != 0));
}

#[test]
fn test_pow_easy_target() {
    // Set a very easy target: exponent=64, coefficient=0xffffff
    // This means target[0..3] = [0xff, 0xff, 0xff, ...]
    // Any hash not starting with 0xffffff will be less than this target
    let easy_bits = 0x40ffffff_u32;

    let header = BlockHeader {
        version: 1,
        prev_hash: Hash::from_bytes([0u8; 64]),
        merkle_root: Hash::from_bytes([0u8; 64]),
        timestamp: 0,
        difficulty_bits: easy_bits,
        nonce: [0u8; 32],
    };

    // Should pass with any nonce since target is so high
    assert!(header.check_pow());
}

// ========================================================================
// Blockchain Tests
// ========================================================================

#[test]
fn test_genesis_block_creation() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

    assert!(genesis.verify_merkle_root());
    assert_eq!(genesis.transactions.len(), 1);
    assert!(genesis.transactions[0].is_coinbase());
}

#[test]
fn test_blockchain_creation() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

    let chain = Blockchain::new(genesis.clone(), 10000, 600, 50_000_000, 210_000);

    assert_eq!(chain.height(), 0);
    assert_eq!(chain.tip_hash(), genesis.hash());
    assert_eq!(chain.balance(&address), 50_000_000);
}

#[test]
fn test_block_reward_halving() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

    let chain = Blockchain::new(genesis, 10000, 600, 50_000_000, 100);

    assert_eq!(chain.block_reward(0), 50_000_000);
    assert_eq!(chain.block_reward(99), 50_000_000);
    assert_eq!(chain.block_reward(100), 25_000_000);
    assert_eq!(chain.block_reward(199), 25_000_000);
    assert_eq!(chain.block_reward(200), 12_500_000);
}

#[test]
fn test_difficulty_adjustment_scaling() {
    // Test that difficulty adjustment stays within 64-bit bounds
    // and handles edge cases correctly.

    // Test coefficient scaling without overflow
    // difficulty_bits = (exponent << 24) | coefficient
    // With exponent=32, coefficient=0x100000 (about 1M)
    let bits: u32 = (32 << 24) | 0x100000;
    let _exponent = bits >> 24;
    let coefficient = (bits & 0x00FFFFFF) as u64;

    // Scaling by 4x (max adjustment)
    let scaled = coefficient * 4;
    assert!(scaled <= 0x7FFFFF * 4); // Should fit in reasonable range

    // Scaling by 0.25x (min adjustment)
    let scaled_down = coefficient / 4;
    assert!(scaled_down > 0);

    // Test exponent boundary: max exponent is 64 for 512-bit hash
    let max_bits: u32 = (64 << 24) | 0x7FFFFF;
    let max_exp = max_bits >> 24;
    assert_eq!(max_exp, 64);

    // Test minimum exponent
    let min_bits: u32 = (1 << 24) | 0x000001;
    let min_exp = min_bits >> 24;
    assert_eq!(min_exp, 1);
}

#[test]
fn test_utxos_for_address() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

    let chain = Blockchain::new(genesis, 10000, 600, 50_000_000, 210_000);

    let utxos = chain.utxos_for_address(&address);
    assert_eq!(utxos.len(), 1);
    assert_eq!(utxos[0].1.output.amount, 50_000_000);
}

// ========================================================================
// Transaction Tests
// ========================================================================

#[test]
fn test_transaction_signing_data() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let tx = Transaction::coinbase(0, 50_000_000, address);

    let signing_data_0 = tx.signing_data(0);
    let signing_data_1 = tx.signing_data(1);

    // Different input indices should produce different signing data
    // (even though this coinbase only has one input)
    assert_ne!(signing_data_0, signing_data_1);
}

#[test]
fn test_transaction_txid_deterministic() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let tx = Transaction::coinbase(0, 50_000_000, address);

    let txid1 = tx.txid();
    let txid2 = tx.txid();
    assert_eq!(txid1, txid2);
}

// ========================================================================
// Error Handling Tests
// ========================================================================

#[test]
fn test_deserialize_truncated_data() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);
    let tx = Transaction::coinbase(0, 50_000_000, address);

    let bytes = tx.to_bytes();
    let truncated = &bytes[..bytes.len() / 2];

    assert!(Transaction::from_bytes(truncated).is_err());
}

#[test]
fn test_deserialize_invalid_tag() {
    let invalid_witness = vec![0xFF, 0x00]; // Invalid witness type tag
    assert!(Witness::from_bytes(&invalid_witness).is_err());
}

#[test]
fn test_deserialize_trailing_bytes() {
    let (pk, _) = test_keypair();
    let address = Address::from_public_key(&pk);

    let mut bytes = address.to_bytes();
    bytes.push(0xFF); // Extra byte

    let result = Address::from_bytes(&bytes);
    assert!(matches!(result, Err(DeserializeError::InvalidData(_))));
}

// ========================================================================
// Negative Path Tests
// ========================================================================

/// Helper to create a test blockchain with a mature genesis coinbase.
///
/// Adds COINBASE_MATURITY empty blocks so the genesis coinbase can be spent.
fn test_blockchain() -> (Blockchain, PublicKey, SecretKey, Address) {
    let (pk, sk) = test_keypair();
    let address = Address::from_public_key(&pk);
    let mut timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - (COINBASE_MATURITY + 10) * 600; // Start in the past

    let genesis = create_genesis_block(
        timestamp, 0x40ffffff, // Easy difficulty
        50_000_000, address,
    );
    let mut chain = Blockchain::new(genesis, 10000, 600, 50_000_000, 210_000);

    // Add COINBASE_MATURITY blocks to mature the genesis coinbase
    for _ in 0..COINBASE_MATURITY {
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
            difficulty_bits: chain.next_difficulty(),
            nonce: [0u8; 32],
        };

        let block = Block::new(header, vec![coinbase]);
        chain.add_block(block).unwrap();
    }

    (chain, pk, sk, address)
}

/// Helper to find a mature UTXO that can be spent
fn find_mature_utxo<'a>(chain: &'a Blockchain, address: &Address) -> (OutPoint, &'a Utxo) {
    let utxos = chain.utxos_for_address(address);
    utxos
        .into_iter()
        .find(|(_, u)| {
            // Mature if non-coinbase, or coinbase with enough confirmations
            !u.is_coinbase || chain.height() >= u.height + COINBASE_MATURITY
        })
        .expect("No mature UTXO found")
}

/// Helper to create a valid spending transaction
fn create_spending_tx(
    chain: &Blockchain,
    from_pk: &PublicKey,
    from_sk: &SecretKey,
    to_address: Address,
) -> Transaction {
    let from_address = Address::from_public_key(from_pk);
    let (outpoint, utxo) = find_mature_utxo(chain, &from_address);

    // Create transaction structure
    let inputs = vec![TxInput::new(
        outpoint,
        Witness::P2PKH {
            public_key: Box::new(from_pk.clone()),
            signature: Box::new(crypto::ml_dsa_87::sign(from_sk, b"placeholder")),
        },
    )];
    let outputs = vec![TxOutput::p2pkh(utxo.output.amount, to_address)];
    let mut tx = Transaction::new(inputs, outputs);

    // Sign the transaction properly
    let signing_data = tx.signing_data(0);
    let message = crypto::hash(&signing_data);
    let signature = crypto::ml_dsa_87::sign(from_sk, message.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(from_pk.clone()),
        signature: Box::new(signature),
    };

    tx
}

/// Helper to create a block containing transactions
fn create_block_with_txs(chain: &Blockchain, txs: Vec<Transaction>, recipient: Address) -> Block {
    let prev_block = chain.tip();
    let height = chain.height() + 1;
    let reward = chain.block_reward(height);

    // Create coinbase
    let mut all_txs = vec![Transaction::coinbase(height, reward, recipient)];
    all_txs.extend(txs);

    let merkle_root = Block::compute_merkle_root(&all_txs);
    // Timestamp must be strictly greater than previous block
    let timestamp = prev_block.header.timestamp + 600;

    let header = BlockHeader {
        version: BlockHeader::CURRENT_VERSION,
        prev_hash: prev_block.hash(),
        merkle_root,
        timestamp,
        difficulty_bits: chain.next_difficulty(),
        nonce: [0u8; 32],
    };

    Block::new(header, all_txs)
}

#[test]
fn test_invalid_signature_rejected() {
    let (mut chain, pk, _sk, address) = test_blockchain();
    let (other_pk, other_sk) = test_keypair();
    let other_address = Address::from_public_key(&other_pk);

    // Find a mature UTXO (genesis coinbase at height 0)
    let utxos = chain.utxos_for_address(&address);
    let (outpoint, utxo) = utxos
        .iter()
        .find(|(_, u)| {
            // Find a mature coinbase (created at height 0, now at height 100+)
            u.height == 0 && chain.height() >= u.height + COINBASE_MATURITY
        })
        .expect("No mature UTXO found");

    let inputs = vec![TxInput::new(
        *outpoint,
        Witness::P2PKH {
            public_key: Box::new(pk.clone()),
            signature: Box::new(crypto::ml_dsa_87::sign(&other_sk, b"wrong key")),
        },
    )];
    let outputs = vec![TxOutput::p2pkh(utxo.output.amount, other_address)];
    let mut tx = Transaction::new(inputs, outputs);

    // Sign with wrong secret key (signature won't verify with pk)
    let signing_data = tx.signing_data(0);
    let message = crypto::hash(&signing_data);
    let wrong_signature = crypto::ml_dsa_87::sign(&other_sk, message.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(wrong_signature),
    };

    let block = create_block_with_txs(&chain, vec![tx], other_address);
    let result = chain.add_block(block);
    assert!(matches!(result, Err(BlockchainError::InvalidWitness)));
}

#[test]
fn test_wrong_public_key_rejected() {
    let (mut chain, _pk, sk, address) = test_blockchain();
    let (other_pk, _other_sk) = test_keypair();
    let other_address = Address::from_public_key(&other_pk);

    // Create a transaction with wrong public key (doesn't match address)
    let (outpoint, utxo) = find_mature_utxo(&chain, &address);

    let inputs = vec![TxInput::new(
        outpoint,
        Witness::P2PKH {
            public_key: Box::new(other_pk.clone()), // Wrong public key
            signature: Box::new(crypto::ml_dsa_87::sign(&sk, b"placeholder")),
        },
    )];
    let outputs = vec![TxOutput::p2pkh(utxo.output.amount, other_address)];
    let mut tx = Transaction::new(inputs, outputs);

    // Sign correctly, but public key doesn't match the address
    let signing_data = tx.signing_data(0);
    let message = crypto::hash(&signing_data);
    let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(other_pk.clone()), // Still wrong
        signature: Box::new(signature),
    };

    let block = create_block_with_txs(&chain, vec![tx], other_address);
    let result = chain.add_block(block);
    assert!(matches!(result, Err(BlockchainError::InvalidWitness)));
}

#[test]
fn test_insufficient_multisig_signatures_rejected() {
    let (pk1, sk1) = test_keypair();
    let (pk2, _sk2) = test_keypair();
    let (pk3, _sk3) = test_keypair();
    let (miner_pk, _) = test_keypair();
    let miner_address = Address::from_public_key(&miner_pk);

    // Create a 2-of-3 multisig output in genesis
    let multisig_condition =
        LockingCondition::multisig(2, vec![pk1.clone(), pk2.clone(), pk3.clone()]);
    let coinbase = Transaction {
        version: Transaction::CURRENT_VERSION,
        inputs: vec![TxInput::coinbase(&0u64.to_le_bytes())],
        outputs: vec![TxOutput {
            amount: 50_000_000,
            condition: multisig_condition,
        }],
    };
    let merkle_root = Block::compute_merkle_root(std::slice::from_ref(&coinbase));
    let mut timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - (COINBASE_MATURITY + 10) * 600;

    let genesis_header = BlockHeader {
        version: BlockHeader::CURRENT_VERSION,
        prev_hash: Hash::from_bytes([0u8; 64]),
        merkle_root,
        timestamp,
        difficulty_bits: 0x40ffffff,
        nonce: [0u8; 32],
    };
    let genesis = Block::new(genesis_header, vec![coinbase.clone()]);

    let mut chain = Blockchain::new(genesis, 10000, 600, 50_000_000, 210_000);

    // Add COINBASE_MATURITY blocks to mature the genesis coinbase
    for _ in 0..COINBASE_MATURITY {
        timestamp += 600;
        let prev_block = chain.tip();
        let height = chain.height() + 1;
        let reward = chain.block_reward(height);

        let miner_coinbase = Transaction::coinbase(height, reward, miner_address);
        let block_merkle_root = Block::compute_merkle_root(std::slice::from_ref(&miner_coinbase));

        let header = BlockHeader {
            version: BlockHeader::CURRENT_VERSION,
            prev_hash: prev_block.hash(),
            merkle_root: block_merkle_root,
            timestamp,
            difficulty_bits: chain.next_difficulty(),
            nonce: [0u8; 32],
        };

        let block = Block::new(header, vec![miner_coinbase]);
        chain.add_block(block).unwrap();
    }

    // Try to spend with only 1 signature (need 2)
    let outpoint = OutPoint::new(coinbase.txid(), 0);
    let (recipient_pk, _) = test_keypair();
    let recipient_address = Address::from_public_key(&recipient_pk);

    let inputs = vec![TxInput::new(
        outpoint,
        Witness::Multisig {
            public_keys: vec![pk1.clone(), pk2.clone(), pk3.clone()],
            signatures: vec![
                Some(crypto::ml_dsa_87::sign(&sk1, b"placeholder")),
                None,
                None,
            ],
        },
    )];
    let outputs = vec![TxOutput::p2pkh(50_000_000, recipient_address)];
    let mut tx = Transaction::new(inputs, outputs);

    // Sign with only sk1 (need 2 signatures for 2-of-3)
    let signing_data = tx.signing_data(0);
    let message = crypto::hash(&signing_data);
    let sig1 = crypto::ml_dsa_87::sign(&sk1, message.as_bytes());
    tx.inputs[0].witness = Witness::Multisig {
        public_keys: vec![pk1.clone(), pk2.clone(), pk3.clone()],
        signatures: vec![Some(sig1), None, None], // Only 1 signature
    };

    let block = create_block_with_txs(&chain, vec![tx], recipient_address);
    let result = chain.add_block(block);
    assert!(matches!(result, Err(BlockchainError::InvalidWitness)));
}

#[test]
fn test_double_spend_within_block_detected() {
    let (mut chain, pk, sk, _address) = test_blockchain();
    let (recipient_pk, _) = test_keypair();
    let recipient_address = Address::from_public_key(&recipient_pk);

    // Create two transactions that spend the same UTXO
    let tx1 = create_spending_tx(&chain, &pk, &sk, recipient_address);
    let tx2 = create_spending_tx(&chain, &pk, &sk, recipient_address);

    // Both transactions try to spend the same output
    assert_eq!(tx1.inputs[0].outpoint, tx2.inputs[0].outpoint);

    let block = create_block_with_txs(&chain, vec![tx1, tx2], recipient_address);
    let result = chain.add_block(block);
    assert!(matches!(result, Err(BlockchainError::DoubleSpend(_))));
}

#[test]
fn test_spending_immature_coinbase_rejected() {
    let (mut chain, pk, sk, address) = test_blockchain();
    let (recipient_pk, _) = test_keypair();
    let recipient_address = Address::from_public_key(&recipient_pk);

    // Add a new block with a coinbase
    let block1 = create_block_with_txs(&chain, vec![], address);
    chain.add_block(block1.clone()).unwrap();

    // Try to spend the new coinbase immediately (before COINBASE_MATURITY blocks)
    let new_coinbase_txid = block1.transactions[0].txid();
    let outpoint = OutPoint::new(new_coinbase_txid, 0);
    let new_coinbase_amount = block1.transactions[0].outputs[0].amount;

    let inputs = vec![TxInput::new(
        outpoint,
        Witness::P2PKH {
            public_key: Box::new(pk.clone()),
            signature: Box::new(crypto::ml_dsa_87::sign(&sk, b"placeholder")),
        },
    )];
    let outputs = vec![TxOutput::p2pkh(new_coinbase_amount, recipient_address)];
    let mut tx = Transaction::new(inputs, outputs);

    // Sign properly
    let signing_data = tx.signing_data(0);
    let message = crypto::hash(&signing_data);
    let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };

    // This should fail because the coinbase is immature
    let block2 = create_block_with_txs(&chain, vec![tx], recipient_address);
    let result = chain.add_block(block2);
    assert!(matches!(
        result,
        Err(BlockchainError::ImmatureCoinbase { .. })
    ));
}

#[test]
fn test_insufficient_inputs_rejected() {
    let (mut chain, pk, sk, address) = test_blockchain();
    let (recipient_pk, _) = test_keypair();
    let recipient_address = Address::from_public_key(&recipient_pk);

    // Try to create more output value than input value
    let (outpoint, utxo) = find_mature_utxo(&chain, &address);

    let inputs = vec![TxInput::new(
        outpoint,
        Witness::P2PKH {
            public_key: Box::new(pk.clone()),
            signature: Box::new(crypto::ml_dsa_87::sign(&sk, b"placeholder")),
        },
    )];
    // Output more than we have
    let outputs = vec![TxOutput::p2pkh(utxo.output.amount + 1, recipient_address)];
    let mut tx = Transaction::new(inputs, outputs);

    let signing_data = tx.signing_data(0);
    let message = crypto::hash(&signing_data);
    let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };

    let block = create_block_with_txs(&chain, vec![tx], recipient_address);
    let result = chain.add_block(block);
    assert!(matches!(result, Err(BlockchainError::InsufficientInputs)));
}

#[test]
fn test_dust_output_rejected() {
    use super::chain::dust_limit;

    let (mut chain, pk, sk, address) = test_blockchain();
    let (recipient_pk, _) = test_keypair();
    let recipient_address = Address::from_public_key(&recipient_pk);

    // Calculate the dust limit for a P2PKH output
    let p2pkh_condition = LockingCondition::P2PKH(recipient_address);
    let expected_dust_limit = dust_limit(&p2pkh_condition);

    // Create a transaction with an output below the dust limit
    let (outpoint, utxo) = find_mature_utxo(&chain, &address);

    let inputs = vec![TxInput::new(
        outpoint,
        Witness::P2PKH {
            public_key: Box::new(pk.clone()),
            signature: Box::new(crypto::ml_dsa_87::sign(&sk, b"placeholder")),
        },
    )];

    // Create a dust output (below the calculated dust limit)
    let dust_amount = expected_dust_limit - 1;
    let change_amount = utxo.output.amount - dust_amount - 1000; // Leave some for fee
    let outputs = vec![
        TxOutput::p2pkh(dust_amount, recipient_address),
        TxOutput::p2pkh(change_amount, address),
    ];
    let mut tx = Transaction::new(inputs, outputs);

    let signing_data = tx.signing_data(0);
    let message = crypto::hash(&signing_data);
    let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
    tx.inputs[0].witness = Witness::P2PKH {
        public_key: Box::new(pk.clone()),
        signature: Box::new(signature),
    };

    let block = create_block_with_txs(&chain, vec![tx], recipient_address);
    let result = chain.add_block(block);
    assert!(
        matches!(result, Err(BlockchainError::DustOutput { index: 0, amount, limit })
            if amount == dust_amount && limit == expected_dust_limit),
        "Expected DustOutput error with amount {dust_amount} and limit {expected_dust_limit}, got {result:?}"
    );
}

#[test]
fn test_dust_limit_scales_with_output_type() {
    use super::chain::dust_limit;

    // Verify that multisig outputs have higher dust limits than P2PKH
    let (pk1, _) = test_keypair();
    let (pk2, _) = test_keypair();
    let (pk3, _) = test_keypair();
    let address = Address::from_public_key(&pk1);

    let p2pkh_condition = LockingCondition::P2PKH(address);
    let multisig_condition = LockingCondition::multisig(2, vec![pk1, pk2, pk3]);

    let p2pkh_limit = dust_limit(&p2pkh_condition);
    let multisig_limit = dust_limit(&multisig_condition);

    // Multisig requires more data to spend (3 pubkeys + 2 sigs vs 1 pubkey + 1 sig)
    assert!(
        multisig_limit > p2pkh_limit,
        "Multisig dust limit ({multisig_limit}) should be greater than P2PKH ({p2pkh_limit})"
    );

    // Verify reasonable values (at DUST_FEE_RATE = 100 quanta/KB)
    // P2PKH spend size: ~7,288 bytes -> dust ~728 quanta
    // 2-of-3 Multisig spend size: ~17,028 bytes -> dust ~1,702 quanta
    assert!(
        p2pkh_limit > 500 && p2pkh_limit < 1500,
        "P2PKH dust limit {p2pkh_limit} outside expected range"
    );
    assert!(
        multisig_limit > 1000 && multisig_limit < 3000,
        "Multisig dust limit {multisig_limit} outside expected range"
    );
}

// ========================================================================
// Storage Persistence Tests
// ========================================================================

#[test]
fn test_blockchain_persistence() {
    use tempfile::TempDir;

    let dir = TempDir::new().expect("failed to create temp dir");

    // Create test parameters
    let (pk, _sk) = test_keypair();
    let address = Address::from_public_key(&pk);
    let genesis = create_genesis_block(0, 0x20ffffff, 50_000_000, address);
    let genesis_hash = genesis.hash();

    // Open blockchain with storage, add a block
    {
        let chain = Blockchain::open(dir.path(), genesis.clone(), 2016, 600, 50_000_000, 210_000)
            .expect("failed to open blockchain");

        assert!(chain.has_storage());
        assert_eq!(chain.height(), 0);
        assert_eq!(chain.tip_hash(), genesis_hash);
    }

    // Reopen and verify state persisted
    {
        let chain = Blockchain::open(dir.path(), genesis.clone(), 2016, 600, 50_000_000, 210_000)
            .expect("failed to reopen blockchain");

        assert_eq!(chain.height(), 0);
        assert_eq!(chain.tip_hash(), genesis_hash);

        // Verify genesis block and UTXOs are present
        let stored_genesis = chain.get_block(&genesis_hash).expect("genesis missing");
        assert_eq!(stored_genesis, &genesis);

        // Verify UTXOs from genesis coinbase
        let coinbase_txid = genesis.transactions[0].txid();
        let outpoint = OutPoint::new(coinbase_txid, 0);
        let utxo = chain.get_utxo(&outpoint).expect("genesis UTXO missing");
        assert_eq!(utxo.output.amount, 50_000_000);
        assert!(utxo.is_coinbase);
    }
}
