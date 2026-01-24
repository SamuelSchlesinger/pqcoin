//! Transaction mempool.
//!
//! Holds unconfirmed transactions waiting to be included in blocks.

use crate::blockchain::{Blockchain, OutPoint, Serialize, Transaction, TxOutput};
use crate::crypto::Hash;
use std::collections::{HashMap, HashSet};

/// Maximum transactions in mempool.
pub const MAX_MEMPOOL_SIZE: usize = 5000;

/// Transaction mempool.
#[derive(Clone)]
pub struct Mempool {
    /// Transactions indexed by txid.
    txs: HashMap<Hash, Transaction>,
    /// Track which outpoints are spent by mempool txs (to detect conflicts).
    spent_outpoints: HashMap<OutPoint, Hash>,
}

impl Mempool {
    pub fn new() -> Self {
        Self {
            txs: HashMap::new(),
            spent_outpoints: HashMap::new(),
        }
    }

    /// Number of transactions in the mempool.
    pub fn len(&self) -> usize {
        self.txs.len()
    }

    /// Check if mempool is empty.
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    /// Check if a transaction is in the mempool.
    pub fn contains(&self, txid: &Hash) -> bool {
        self.txs.contains_key(txid)
    }

    /// Get a transaction by txid.
    pub fn get(&self, txid: &Hash) -> Option<&Transaction> {
        self.txs.get(txid)
    }

    /// Add a transaction to the mempool.
    ///
    /// Returns true if added, false if already present or conflicts.
    pub fn add(&mut self, tx: Transaction, blockchain: &Blockchain) -> Result<bool, MempoolError> {
        let txid = tx.txid();

        // Already in mempool?
        if self.txs.contains_key(&txid) {
            return Ok(false);
        }

        // Mempool full?
        if self.txs.len() >= MAX_MEMPOOL_SIZE {
            return Err(MempoolError::Full);
        }

        // Coinbase not allowed in mempool
        if tx.is_coinbase() {
            return Err(MempoolError::CoinbaseNotAllowed);
        }

        // Check for double-spends within mempool
        for input in &tx.inputs {
            if let Some(conflicting_txid) = self.spent_outpoints.get(&input.outpoint) {
                return Err(MempoolError::DoubleSpend(*conflicting_txid));
            }
        }

        // Validate transaction against blockchain
        if let Err(e) = validate_mempool_tx(&tx, blockchain, self) {
            return Err(e);
        }

        // Add to mempool
        for input in &tx.inputs {
            self.spent_outpoints.insert(input.outpoint, txid);
        }
        self.txs.insert(txid, tx);

        Ok(true)
    }

    /// Remove a transaction from the mempool.
    pub fn remove(&mut self, txid: &Hash) -> Option<Transaction> {
        if let Some(tx) = self.txs.remove(txid) {
            for input in &tx.inputs {
                self.spent_outpoints.remove(&input.outpoint);
            }
            Some(tx)
        } else {
            None
        }
    }

    /// Remove transactions that are now in a block.
    pub fn remove_confirmed(&mut self, txs: &[Transaction]) {
        for tx in txs {
            self.remove(&tx.txid());
        }
    }

    /// Remove transactions that conflict with a block (double-spends).
    pub fn remove_conflicts(&mut self, txs: &[Transaction]) {
        let mut to_remove = HashSet::new();

        for tx in txs {
            for input in &tx.inputs {
                if let Some(conflicting_txid) = self.spent_outpoints.get(&input.outpoint) {
                    to_remove.insert(*conflicting_txid);
                }
            }
        }

        for txid in to_remove {
            self.remove(&txid);
        }
    }

    /// Get transactions for block template (up to max_txs), sorted by fee rate.
    ///
    /// Requires blockchain reference to look up input values for fee calculation.
    pub fn get_block_txs(&self, max_txs: usize) -> Vec<Transaction> {
        // Note: This is a simple version that doesn't require blockchain reference.
        // For proper fee calculation, use get_block_txs_with_fees.
        self.txs.values().take(max_txs).cloned().collect()
    }

    /// Get transactions for block template sorted by fee rate (fee per byte).
    ///
    /// Transactions with higher fee rates are selected first.
    pub fn get_block_txs_with_fees(&self, max_txs: usize, blockchain: &Blockchain) -> Vec<Transaction> {
        let mut txs_with_fee_rate: Vec<(&Transaction, f64)> = self
            .txs
            .values()
            .filter_map(|tx| {
                let fee = self.calculate_fee(tx, blockchain)?;
                let size = tx.to_bytes().len();
                if size == 0 {
                    return None;
                }
                let fee_rate = fee as f64 / size as f64;
                Some((tx, fee_rate))
            })
            .collect();

        // Sort by fee rate descending (highest fee rate first)
        txs_with_fee_rate.sort_by(|a, b| {
            b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
        });

        txs_with_fee_rate
            .into_iter()
            .take(max_txs)
            .map(|(tx, _)| tx.clone())
            .collect()
    }

    /// Calculate the fee for a transaction.
    ///
    /// Returns None if any input is missing (which shouldn't happen for valid mempool txs).
    fn calculate_fee(&self, tx: &Transaction, blockchain: &Blockchain) -> Option<u64> {
        let mut input_sum = 0u64;

        for input in &tx.inputs {
            let utxo = blockchain.get_utxo(&input.outpoint)?;
            input_sum = input_sum.checked_add(utxo.output.amount)?;
        }

        let output_sum: u64 = tx.outputs.iter().map(|o| o.amount).sum();

        input_sum.checked_sub(output_sum)
    }

    /// Get all transaction IDs.
    pub fn txids(&self) -> Vec<Hash> {
        self.txs.keys().copied().collect()
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new()
    }
}

/// Mempool errors.
#[derive(Debug, Clone)]
pub enum MempoolError {
    Full,
    CoinbaseNotAllowed,
    DoubleSpend(Hash),
    MissingInput(OutPoint),
    InvalidSignature,
    InsufficientFunds,
    /// Attempted to spend an immature coinbase output.
    ImmatureCoinbase(OutPoint),
    /// Transaction witness type doesn't match output locking condition.
    InvalidWitness,
}

impl std::fmt::Display for MempoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MempoolError::Full => write!(f, "mempool full"),
            MempoolError::CoinbaseNotAllowed => write!(f, "coinbase transactions not allowed"),
            MempoolError::DoubleSpend(txid) => write!(f, "conflicts with tx {}", txid),
            MempoolError::MissingInput(op) => write!(f, "missing input {:?}", op),
            MempoolError::InvalidSignature => write!(f, "invalid signature"),
            MempoolError::InsufficientFunds => write!(f, "insufficient funds"),
            MempoolError::ImmatureCoinbase(op) => write!(f, "immature coinbase output: {:?}", op),
            MempoolError::InvalidWitness => write!(f, "invalid witness type"),
        }
    }
}

impl std::error::Error for MempoolError {}

/// Validate a transaction for mempool inclusion.
fn validate_mempool_tx(
    tx: &Transaction,
    blockchain: &Blockchain,
    _mempool: &Mempool,
) -> Result<(), MempoolError> {
    let mut total_in = 0u64;
    let mut total_out = 0u64;
    let current_height = blockchain.height();

    for (input_index, input) in tx.inputs.iter().enumerate() {
        // Check if UTXO exists in blockchain
        let utxo = blockchain
            .get_utxo(&input.outpoint)
            .ok_or(MempoolError::MissingInput(input.outpoint))?;

        // Check coinbase maturity
        if utxo.is_coinbase && current_height < utxo.height + crate::constants::COINBASE_MATURITY {
            return Err(MempoolError::ImmatureCoinbase(input.outpoint));
        }

        // Validate witness with explicit input index
        if !validate_witness(&input.witness, &utxo.output, tx, input_index) {
            return Err(MempoolError::InvalidSignature);
        }

        total_in += utxo.output.amount;
    }

    for output in &tx.outputs {
        total_out += output.amount;
    }

    if total_out > total_in {
        return Err(MempoolError::InsufficientFunds);
    }

    Ok(())
}

/// Validate a witness against an output's locking condition.
///
/// Uses explicit input_index to correctly handle multi-input transactions from the same address.
fn validate_witness(
    witness: &crate::blockchain::Witness,
    output: &TxOutput,
    tx: &Transaction,
    input_index: usize,
) -> bool {
    use crate::blockchain::{LockingCondition, Witness};
    use crate::crypto::{hash, ml_dsa_87};

    match (&output.condition, witness) {
        (LockingCondition::P2PKH(addr), Witness::P2PKH { public_key, signature }) => {
            // Check public key hashes to address
            let pk_hash = hash(public_key.as_ref());
            if pk_hash != *addr.as_hash() {
                return false;
            }

            // Compute signing data using the explicit input index
            let signing_data = tx.signing_data(input_index);
            // Hash the signing data before verification (matches blockchain verification)
            let message = hash(&signing_data);
            ml_dsa_87::verify(public_key, message.as_bytes(), signature)
        }
        (
            LockingCondition::Multisig { threshold, public_keys },
            Witness::Multisig {
                public_keys: witness_keys,
                signatures,
            },
        ) => {
            // Verify the public keys match
            if witness_keys.len() != public_keys.len() {
                return false;
            }
            for (wk, pk) in witness_keys.iter().zip(public_keys.iter()) {
                if wk.to_bytes() != pk.to_bytes() {
                    return false;
                }
            }

            // Compute signing data
            let signing_data = tx.signing_data(input_index);
            let message = hash(&signing_data);

            // Count valid signatures
            let mut valid_sigs = 0u8;
            for (i, sig_opt) in signatures.iter().enumerate() {
                if let Some(sig) = sig_opt {
                    if i < public_keys.len()
                        && ml_dsa_87::verify(&public_keys[i], message.as_bytes(), sig)
                    {
                        valid_sigs += 1;
                    }
                }
            }

            valid_sigs >= *threshold
        }
        _ => false,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::{
        create_genesis_block, Address, Block, BlockHeader, LockingCondition, OutPoint,
        TxInput, TxOutput, Witness,
    };
    use crate::crypto::{hash, ml_dsa_87};

    // Helper to create a test blockchain with mature coinbase
    fn test_blockchain() -> (Blockchain, crate::crypto::PublicKey, crate::crypto::SecretKey, Address) {
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
            let merkle_root = Block::compute_merkle_root(&[coinbase.clone()]);

            let header = BlockHeader {
                version: BlockHeader::CURRENT_VERSION,
                prev_hash: prev_block.hash(),
                merkle_root,
                timestamp,
                difficulty_bits: prev_block.header.difficulty_bits,
                nonce: 0,
            };

            let block = Block::new(header, vec![coinbase]);
            chain.add_block(block).unwrap();
        }

        (chain, pk, sk, address)
    }

    // Helper to create a valid spending transaction
    fn create_spending_tx(
        chain: &Blockchain,
        pk: &crate::crypto::PublicKey,
        sk: &crate::crypto::SecretKey,
        address: Address,
    ) -> Transaction {
        let utxos = chain.utxos_for_address(&address);
        let (outpoint, utxo) = utxos
            .into_iter()
            .find(|(_, u)| {
                !u.is_coinbase
                    || chain.height() >= u.height + crate::constants::COINBASE_MATURITY
            })
            .expect("no spendable UTXO");

        let mut tx = Transaction::new(
            vec![TxInput::new(
                outpoint,
                Witness::P2PKH {
                    public_key: pk.clone(),
                    signature: ml_dsa_87::sign(sk, &[0u8; 64]), // placeholder
                },
            )],
            vec![TxOutput::p2pkh(utxo.output.amount - 1000, address)], // 1000 fee
        );

        // Sign properly
        let signing_data = tx.signing_data(0);
        let message_hash = hash(&signing_data);
        let signature = ml_dsa_87::sign(sk, message_hash.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: pk.clone(),
            signature,
        };

        tx
    }

    #[test]
    fn test_mempool_add_valid_transaction() {
        let (chain, pk, sk, address) = test_blockchain();
        let mut mempool = Mempool::new();

        let tx = create_spending_tx(&chain, &pk, &sk, address);
        let result = mempool.add(tx, &chain);

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), true);
        assert_eq!(mempool.len(), 1);
    }

    #[test]
    fn test_mempool_double_add_same_tx() {
        let (chain, pk, sk, address) = test_blockchain();
        let mut mempool = Mempool::new();

        let tx = create_spending_tx(&chain, &pk, &sk, address);
        let txid = tx.txid();

        // Add first time
        assert!(mempool.add(tx.clone(), &chain).unwrap());
        // Add second time - should return false (already present)
        assert!(!mempool.add(tx, &chain).unwrap());
        assert_eq!(mempool.len(), 1);
        assert!(mempool.contains(&txid));
    }

    #[test]
    fn test_mempool_double_spend_detection() {
        let (chain, pk, sk, address) = test_blockchain();
        let mut mempool = Mempool::new();

        // Create first transaction spending the UTXO
        let tx1 = create_spending_tx(&chain, &pk, &sk, address);
        let tx1_id = tx1.txid();

        // Create second transaction spending the same UTXO (different output)
        let utxos = chain.utxos_for_address(&address);
        let (outpoint, utxo) = utxos
            .into_iter()
            .find(|(_, u)| {
                !u.is_coinbase
                    || chain.height() >= u.height + crate::constants::COINBASE_MATURITY
            })
            .expect("no spendable UTXO");

        let mut tx2 = Transaction::new(
            vec![TxInput::new(
                outpoint,
                Witness::P2PKH {
                    public_key: pk.clone(),
                    signature: ml_dsa_87::sign(&sk, &[0u8; 64]),
                },
            )],
            vec![TxOutput::p2pkh(utxo.output.amount - 2000, address)], // different amount
        );

        let signing_data = tx2.signing_data(0);
        let message_hash = hash(&signing_data);
        let signature = ml_dsa_87::sign(&sk, message_hash.as_bytes());
        tx2.inputs[0].witness = Witness::P2PKH {
            public_key: pk.clone(),
            signature,
        };

        // Add first transaction
        assert!(mempool.add(tx1, &chain).is_ok());

        // Try to add second (conflicting) transaction
        let result = mempool.add(tx2, &chain);
        assert!(matches!(result, Err(MempoolError::DoubleSpend(id)) if id == tx1_id));
    }

    #[test]
    fn test_mempool_remove_confirmed() {
        let (chain, pk, sk, address) = test_blockchain();
        let mut mempool = Mempool::new();

        let tx = create_spending_tx(&chain, &pk, &sk, address);
        let txid = tx.txid();

        // Add to mempool
        assert!(mempool.add(tx.clone(), &chain).unwrap());
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

        let tx = create_spending_tx(&chain, &pk, &sk, address);
        let spent_outpoint = tx.inputs[0].outpoint;

        // Add to mempool
        assert!(mempool.add(tx.clone(), &chain).unwrap());
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

        // Add a transaction
        let tx = create_spending_tx(&chain, &pk, &sk, address);
        mempool.add(tx, &chain).unwrap();

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

        let coinbase = Transaction::coinbase(100, 50_000_000, address);
        let result = mempool.add(coinbase, &chain);

        assert!(matches!(result, Err(MempoolError::CoinbaseNotAllowed)));
    }

    #[test]
    fn test_mempool_rejects_missing_input() {
        let (chain, pk, sk, _address) = test_blockchain();
        let mut mempool = Mempool::new();

        // Create transaction referencing non-existent UTXO
        let fake_outpoint = OutPoint::new(hash(b"fake tx"), 0);
        let (other_pk, _) = ml_dsa_87::keygen();
        let other_address = Address::from_public_key(&other_pk);

        let mut tx = Transaction::new(
            vec![TxInput::new(
                fake_outpoint,
                Witness::P2PKH {
                    public_key: pk.clone(),
                    signature: ml_dsa_87::sign(&sk, &[0u8; 64]),
                },
            )],
            vec![TxOutput::p2pkh(1000, other_address)],
        );

        let signing_data = tx.signing_data(0);
        let message_hash = hash(&signing_data);
        let signature = ml_dsa_87::sign(&sk, message_hash.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: pk.clone(),
            signature,
        };

        let result = mempool.add(tx, &chain);
        assert!(matches!(result, Err(MempoolError::MissingInput(_))));
    }

    #[test]
    fn test_mempool_txids() {
        let (chain, pk, sk, address) = test_blockchain();
        let mut mempool = Mempool::new();

        let tx = create_spending_tx(&chain, &pk, &sk, address);
        let txid = tx.txid();

        mempool.add(tx, &chain).unwrap();

        let txids = mempool.txids();
        assert_eq!(txids.len(), 1);
        assert!(txids.contains(&txid));
    }
}
