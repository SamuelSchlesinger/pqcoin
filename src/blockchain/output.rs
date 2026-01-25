//! TxOutput type representing a transaction output.

use super::address::Address;
use super::locking::LockingCondition;
use super::serialize::{Deserialize, DeserializeError, Serialize, read_u64, write_u64};
use crate::crypto::PublicKey;

/// A transaction output, representing spendable value.
///
/// Each output has an amount (in the smallest unit, "quanta") and a locking
/// condition that specifies who can spend it.
///
/// # Serialization Format
///
/// | Field     | Size     | Description                      |
/// |-----------|----------|----------------------------------|
/// | amount    | 8 bytes  | Value in quanta (little-endian)  |
/// | condition | variable | Locking condition                |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxOutput {
    /// The amount in quanta (smallest unit, like satoshis in Bitcoin).
    pub amount: u64,
    /// The condition required to spend this output.
    pub condition: LockingCondition,
}

impl TxOutput {
    /// Create a new P2PKH output.
    pub fn p2pkh(amount: u64, address: Address) -> Self {
        Self {
            amount,
            condition: LockingCondition::P2PKH(address),
        }
    }

    /// Create a new multisig output.
    pub fn multisig(amount: u64, threshold: u8, public_keys: Vec<PublicKey>) -> Self {
        Self {
            amount,
            condition: LockingCondition::multisig(threshold, public_keys),
        }
    }
}

impl Serialize for TxOutput {
    fn serialize(&self, buf: &mut Vec<u8>) {
        write_u64(buf, self.amount);
        self.condition.serialize(buf);
    }
}

impl Deserialize for TxOutput {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (amount, data) = read_u64(data)?;
        let (condition, data) = LockingCondition::deserialize(data)?;
        Ok((TxOutput { amount, condition }, data))
    }
}
