//! Block type containing header and transactions.

use super::header::BlockHeader;
use super::serialize::{Deserialize, DeserializeError, Serialize, read_var_int, write_var_int};
use super::transaction::Transaction;
use crate::constants::MAX_BLOCK_TXS;
use crate::crypto::{self, Hash};

/// A complete block containing a header and transactions.
///
/// Blocks are the fundamental unit of the blockchain. Each block contains:
/// - A header with metadata and proof-of-work
/// - A list of transactions, starting with a coinbase transaction
///
/// # Merkle Root
///
/// The merkle root in the header commits to all transactions in the block.
/// It is computed by building a binary hash tree of transaction IDs.
///
/// # Serialization Format
///
/// | Field        | Size     | Description                    |
/// |--------------|----------|--------------------------------|
/// | header       | 152 bytes| Block header                   |
/// | num_txs      | varint   | Number of transactions         |
/// | transactions | variable | Transaction data               |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The block header.
    pub header: BlockHeader,
    /// The transactions in this block.
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Create a new block with the given header and transactions.
    pub fn new(header: BlockHeader, transactions: Vec<Transaction>) -> Self {
        Self {
            header,
            transactions,
        }
    }

    /// Compute the merkle root of the transactions.
    ///
    /// The merkle tree is a binary tree where:
    /// - Leaves are transaction IDs (hashes)
    /// - Internal nodes are hashes of their concatenated children
    /// - If a level has an odd number of nodes, the last node is duplicated
    pub fn compute_merkle_root(transactions: &[Transaction]) -> Hash {
        if transactions.is_empty() {
            return Hash::from_bytes([0u8; 64]);
        }

        let mut hashes: Vec<Hash> = transactions.iter().map(|tx| tx.txid()).collect();

        while hashes.len() > 1 {
            let mut next_level = Vec::with_capacity((hashes.len() + 1) / 2);

            for chunk in hashes.chunks(2) {
                let combined = if chunk.len() == 2 {
                    crypto::hash_many(&[chunk[0].as_bytes(), chunk[1].as_bytes()])
                } else {
                    // Odd number: duplicate the last hash
                    crypto::hash_many(&[chunk[0].as_bytes(), chunk[0].as_bytes()])
                };
                next_level.push(combined);
            }

            hashes = next_level;
        }

        hashes[0]
    }

    /// Get the block hash (hash of the header).
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }

    /// Verify that the merkle root in the header matches the transactions.
    pub fn verify_merkle_root(&self) -> bool {
        self.header.merkle_root == Self::compute_merkle_root(&self.transactions)
    }

    /// Get the coinbase transaction (first transaction in the block).
    pub fn coinbase(&self) -> Option<&Transaction> {
        self.transactions.first()
    }

    /// Get the block height from the coinbase transaction.
    ///
    /// Returns `None` if the coinbase transaction is malformed.
    pub fn height(&self) -> Option<u64> {
        // The height is encoded in the coinbase input data
        // For simplicity, we don't extract it here; the blockchain tracks height
        None
    }
}

impl Serialize for Block {
    fn serialize(&self, buf: &mut Vec<u8>) {
        self.header.serialize(buf);
        write_var_int(buf, self.transactions.len() as u64);
        for tx in &self.transactions {
            tx.serialize(buf);
        }
    }
}

impl Deserialize for Block {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (header, data) = BlockHeader::deserialize(data)?;
        let (num_txs, data) = read_var_int(data)?;
        if num_txs > MAX_BLOCK_TXS as u64 {
            return Err(DeserializeError::LengthOverflow);
        }
        let mut transactions = Vec::with_capacity(num_txs as usize);
        let mut data = data;
        for _ in 0..num_txs {
            let (tx, rest) = Transaction::deserialize(data)?;
            transactions.push(tx);
            data = rest;
        }
        Ok((
            Block {
                header,
                transactions,
            },
            data,
        ))
    }
}
