//! TxInput type representing a transaction input.

use super::outpoint::OutPoint;
use super::witness::Witness;
use super::serialize::{Deserialize, DeserializeError, Serialize};

/// A transaction input, referencing a previous output to spend.
///
/// Each input references a specific output from a previous transaction via its
/// [`OutPoint`] and provides a [`Witness`] proving authorization to spend it.
///
/// # Coinbase Transactions
///
/// The first input of a coinbase transaction (block reward) has a null outpoint
/// and an arbitrary witness. This input creates new coins rather than spending
/// existing ones.
///
/// # Serialization Format
///
/// | Field    | Size     | Description                     |
/// |----------|----------|---------------------------------|
/// | outpoint | 68 bytes | Reference to previous output    |
/// | witness  | variable | Spending proof                  |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxInput {
    /// Reference to the output being spent.
    pub outpoint: OutPoint,
    /// Witness data proving authorization to spend.
    pub witness: Witness,
}

impl TxInput {
    /// Create a new transaction input.
    pub fn new(outpoint: OutPoint, witness: Witness) -> Self {
        Self { outpoint, witness }
    }

    /// Create a coinbase input with arbitrary data.
    ///
    /// Coinbase inputs have a null outpoint and a [`Witness::Coinbase`] that
    /// carries arbitrary data (typically the block height and extra nonce).
    /// This avoids the overhead of generating unused cryptographic keys.
    pub fn coinbase(data: &[u8]) -> Self {
        Self {
            outpoint: OutPoint::null(),
            witness: Witness::Coinbase(data.to_vec()),
        }
    }

    /// Check if this is a coinbase input.
    pub fn is_coinbase(&self) -> bool {
        self.outpoint.is_null()
    }
}

impl Serialize for TxInput {
    fn serialize(&self, buf: &mut Vec<u8>) {
        self.outpoint.serialize(buf);
        self.witness.serialize(buf);
    }
}

impl Deserialize for TxInput {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (outpoint, data) = OutPoint::deserialize(data)?;
        let (witness, data) = Witness::deserialize(data)?;
        Ok((TxInput { outpoint, witness }, data))
    }
}
