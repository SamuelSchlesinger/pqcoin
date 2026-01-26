//! Mempool error types.

use crate::blockchain::OutPoint;
use crate::crypto::Hash;

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
    /// Transaction fee rate is below the minimum relay fee.
    FeeTooLow {
        /// The fee rate that was provided.
        got: u64,
        /// The minimum required fee rate.
        min: u64,
    },
    /// Transaction output is below the dust limit.
    DustOutput {
        /// Output index.
        index: usize,
        /// Amount in the output.
        amount: u64,
        /// Minimum dust limit.
        limit: u64,
    },
}

impl std::fmt::Display for MempoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MempoolError::Full => write!(f, "mempool full"),
            MempoolError::CoinbaseNotAllowed => write!(f, "coinbase transactions not allowed"),
            MempoolError::DoubleSpend(txid) => write!(f, "conflicts with tx {txid}"),
            MempoolError::MissingInput(op) => write!(f, "missing input {op:?}"),
            MempoolError::InvalidSignature => write!(f, "invalid signature"),
            MempoolError::InsufficientFunds => write!(f, "insufficient funds"),
            MempoolError::ImmatureCoinbase(op) => write!(f, "immature coinbase output: {op:?}"),
            MempoolError::InvalidWitness => write!(f, "invalid witness type"),
            MempoolError::FeeTooLow { got, min } => {
                write!(f, "fee rate {got} is below minimum {min}")
            }
            MempoolError::DustOutput {
                index,
                amount,
                limit,
            } => {
                write!(
                    f,
                    "output {index} amount {amount} is below dust limit {limit}"
                )
            }
        }
    }
}

impl std::error::Error for MempoolError {}
