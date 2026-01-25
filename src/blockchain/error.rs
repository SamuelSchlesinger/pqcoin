//! Error types for blockchain operations.

use super::outpoint::OutPoint;
use crate::crypto::Hash;
use crate::storage::StorageError;

/// Errors that can occur during blockchain operations.
#[derive(Debug)]
pub enum BlockchainError {
    /// The block's previous hash doesn't match any known block.
    UnknownPreviousBlock,
    /// The block's proof-of-work is invalid.
    InvalidProofOfWork,
    /// The block's merkle root doesn't match the transactions.
    InvalidMerkleRoot,
    /// The block has no transactions.
    EmptyBlock,
    /// The first transaction is not a valid coinbase.
    InvalidCoinbase,
    /// A transaction input references a non-existent output.
    MissingInput(OutPoint),
    /// A transaction input's witness is invalid.
    InvalidWitness,
    /// A transaction's inputs don't cover its outputs.
    InsufficientInputs,
    /// A transaction output has already been spent.
    DoubleSpend(OutPoint),
    /// The block timestamp is invalid.
    InvalidTimestamp,
    /// The block difficulty doesn't match the expected value.
    InvalidDifficulty,
    /// Fee calculation would overflow.
    FeeOverflow,
    /// Block serialized size exceeds maximum.
    BlockTooLarge,
    /// Attempted to spend an immature coinbase output.
    ImmatureCoinbase {
        outpoint: OutPoint,
        current_height: u64,
        maturity_height: u64,
    },
    /// Reorg depth exceeds maximum allowed.
    ReorgTooDeep(u64),
    /// Transaction has too many inputs.
    TooManyInputs(usize),
    /// Transaction has too many outputs.
    TooManyOutputs(usize),
    /// Transaction output is below the dust limit.
    DustOutput {
        index: usize,
        amount: u64,
        limit: u64,
    },
    /// Block hash doesn't match the checkpoint at this height.
    CheckpointMismatch {
        height: u64,
        expected: Box<Hash>,
        got: Box<Hash>,
    },
    /// Storage error.
    Storage(StorageError),
}

impl From<StorageError> for BlockchainError {
    fn from(err: StorageError) -> Self {
        BlockchainError::Storage(err)
    }
}

impl std::fmt::Display for BlockchainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockchainError::UnknownPreviousBlock => write!(f, "unknown previous block"),
            BlockchainError::InvalidProofOfWork => write!(f, "invalid proof of work"),
            BlockchainError::InvalidMerkleRoot => write!(f, "invalid merkle root"),
            BlockchainError::EmptyBlock => write!(f, "block has no transactions"),
            BlockchainError::InvalidCoinbase => write!(f, "invalid coinbase transaction"),
            BlockchainError::MissingInput(op) => write!(f, "missing input: {op:?}"),
            BlockchainError::InvalidWitness => write!(f, "invalid witness"),
            BlockchainError::InsufficientInputs => write!(f, "insufficient inputs"),
            BlockchainError::DoubleSpend(op) => write!(f, "double spend: {op:?}"),
            BlockchainError::InvalidTimestamp => write!(f, "invalid timestamp"),
            BlockchainError::InvalidDifficulty => write!(f, "invalid difficulty"),
            BlockchainError::FeeOverflow => write!(f, "fee calculation overflow"),
            BlockchainError::BlockTooLarge => write!(f, "block exceeds maximum size"),
            BlockchainError::ImmatureCoinbase {
                outpoint,
                current_height,
                maturity_height,
            } => {
                write!(
                    f,
                    "immature coinbase: {outpoint:?} (current height {current_height}, matures at {maturity_height})"
                )
            }
            BlockchainError::ReorgTooDeep(depth) => write!(f, "reorg too deep: {depth} blocks"),
            BlockchainError::TooManyInputs(count) => write!(f, "too many inputs: {count}"),
            BlockchainError::TooManyOutputs(count) => write!(f, "too many outputs: {count}"),
            BlockchainError::DustOutput {
                index,
                amount,
                limit,
            } => {
                write!(f, "output {index} is dust: {amount} below limit {limit}")
            }
            BlockchainError::CheckpointMismatch {
                height,
                expected,
                got,
            } => {
                write!(
                    f,
                    "checkpoint mismatch at height {height}: expected {expected}, got {got}"
                )
            }
            BlockchainError::Storage(e) => write!(f, "storage error: {e}"),
        }
    }
}

impl std::error::Error for BlockchainError {}
