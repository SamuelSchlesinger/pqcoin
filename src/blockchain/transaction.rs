//! Transaction type for transferring value.

use crate::constants::{MAX_TX_INPUTS, MAX_TX_OUTPUTS};
use crate::crypto::{self, Hash};
use super::address::Address;
use super::input::TxInput;
use super::output::TxOutput;
use super::serialize::{
    Deserialize, DeserializeError, Serialize,
    read_u32, read_var_int, write_u8, write_u32, write_var_int,
};

/// A transaction transferring value between outputs.
///
/// Transactions consume existing unspent outputs (via inputs) and create new
/// outputs. The sum of input values must equal or exceed the sum of output
/// values; any difference is the transaction fee, claimed by the miner.
///
/// # Transaction ID
///
/// The transaction ID (txid) is the SHA3-512 hash of the serialized transaction.
/// This hash is used to reference the transaction in outpoints.
///
/// # Coinbase Transactions
///
/// The first transaction in each block is a coinbase transaction that creates
/// new coins. It has exactly one input with a null outpoint.
///
/// # Serialization Format
///
/// | Field      | Size     | Description                        |
/// |------------|----------|------------------------------------|
/// | version    | 4 bytes  | Transaction version (little-endian)|
/// | num_inputs | varint   | Number of inputs                   |
/// | inputs     | variable | Transaction inputs                 |
/// | num_outputs| varint   | Number of outputs                  |
/// | outputs    | variable | Transaction outputs                |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transaction {
    /// Transaction format version.
    pub version: u32,
    /// The inputs (outputs being spent).
    pub inputs: Vec<TxInput>,
    /// The outputs (new spendable values).
    pub outputs: Vec<TxOutput>,
}

impl Transaction {
    /// The current transaction version.
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new transaction.
    pub fn new(inputs: Vec<TxInput>, outputs: Vec<TxOutput>) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            inputs,
            outputs,
        }
    }

    /// Create a coinbase transaction.
    ///
    /// # Arguments
    ///
    /// * `block_height` - The height of the block (used in coinbase data)
    /// * `reward` - The block reward amount
    /// * `recipient` - The address to receive the reward
    pub fn coinbase(block_height: u64, reward: u64, recipient: Address) -> Self {
        let coinbase_data = block_height.to_le_bytes();
        Self {
            version: Self::CURRENT_VERSION,
            inputs: vec![TxInput::coinbase(&coinbase_data)],
            outputs: vec![TxOutput::p2pkh(reward, recipient)],
        }
    }

    /// Compute the transaction ID (hash).
    pub fn txid(&self) -> Hash {
        crypto::hash(&self.to_bytes())
    }

    /// Check if this is a coinbase transaction.
    pub fn is_coinbase(&self) -> bool {
        self.inputs.len() == 1 && self.inputs[0].is_coinbase()
    }

    /// Get the total output value.
    pub fn total_output(&self) -> u64 {
        self.outputs.iter().map(|o| o.amount).sum()
    }

    /// Serialize the transaction for signing (excludes witnesses).
    ///
    /// When signing an input, the witness data is excluded to avoid
    /// circular dependencies.
    pub fn signing_data(&self, input_index: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        write_u32(&mut buf, self.version);
        write_var_int(&mut buf, self.inputs.len() as u64);
        for (i, input) in self.inputs.iter().enumerate() {
            input.outpoint.serialize(&mut buf);
            // Include a marker for which input is being signed
            if i == input_index {
                write_u8(&mut buf, 0x01);
            } else {
                write_u8(&mut buf, 0x00);
            }
        }
        write_var_int(&mut buf, self.outputs.len() as u64);
        for output in &self.outputs {
            output.serialize(&mut buf);
        }
        buf
    }
}

impl Serialize for Transaction {
    fn serialize(&self, buf: &mut Vec<u8>) {
        write_u32(buf, self.version);
        write_var_int(buf, self.inputs.len() as u64);
        for input in &self.inputs {
            input.serialize(buf);
        }
        write_var_int(buf, self.outputs.len() as u64);
        for output in &self.outputs {
            output.serialize(buf);
        }
    }
}

impl Deserialize for Transaction {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (version, data) = read_u32(data)?;
        let (num_inputs, data) = read_var_int(data)?;
        if num_inputs > MAX_TX_INPUTS as u64 {
            return Err(DeserializeError::LengthOverflow);
        }
        let mut inputs = Vec::with_capacity(num_inputs as usize);
        let mut data = data;
        for _ in 0..num_inputs {
            let (input, rest) = TxInput::deserialize(data)?;
            inputs.push(input);
            data = rest;
        }
        let (num_outputs, data) = read_var_int(data)?;
        if num_outputs > MAX_TX_OUTPUTS as u64 {
            return Err(DeserializeError::LengthOverflow);
        }
        let mut outputs = Vec::with_capacity(num_outputs as usize);
        let mut data = data;
        for _ in 0..num_outputs {
            let (output, rest) = TxOutput::deserialize(data)?;
            outputs.push(output);
            data = rest;
        }
        Ok((
            Transaction {
                version,
                inputs,
                outputs,
            },
            data,
        ))
    }
}
