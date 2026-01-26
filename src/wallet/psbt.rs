//! Partially Signed Bitcoin Transaction (PSBT) format for pqcoin.
//!
//! This module provides a JSON-serializable format for transactions that require
//! multiple signatures (multisig). It allows coordinating signature collection
//! across multiple wallets.

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::blockchain::{
    Address, LockingCondition, OutPoint, Transaction, TxInput, TxOutput, Witness,
};
use crate::crypto::{self, Hash, PublicKey, Signature};

use super::keys::WalletError;

/// A partially signed transaction.
///
/// This structure holds all the information needed to build a complete transaction,
/// including partial signatures collected from multiple parties.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialTransaction {
    /// Transaction version.
    pub version: u32,
    /// The inputs being spent.
    pub inputs: Vec<PartialInput>,
    /// The outputs being created.
    pub outputs: Vec<PartialOutput>,
    /// Transaction fee in quanta.
    pub fee: u64,
}

/// A partially signed input.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialInput {
    /// The outpoint being spent.
    pub outpoint: OutPointData,
    /// The amount being spent (for display/verification).
    pub amount: u64,
    /// The locking condition of the output being spent.
    pub locking_condition: LockingConditionData,
    /// Partial signatures (indices correspond to public keys in multisig).
    /// For P2PKH, this is a single-element vector.
    pub signatures: Vec<Option<String>>,
}

/// Serializable outpoint data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutPointData {
    /// Transaction ID (hex-encoded).
    pub txid: String,
    /// Output index.
    pub index: u32,
}

impl OutPointData {
    /// Create from an OutPoint.
    pub fn from_outpoint(op: &OutPoint) -> Self {
        Self {
            txid: op.txid.to_hex(),
            index: op.index,
        }
    }

    /// Convert to an OutPoint.
    pub fn to_outpoint(&self) -> Result<OutPoint, WalletError> {
        let txid_bytes = hex::decode(&self.txid)
            .map_err(|e| WalletError::InvalidFormat(format!("invalid txid hex: {e}")))?;

        if txid_bytes.len() != 64 {
            return Err(WalletError::InvalidFormat(format!(
                "txid must be 64 bytes, got {}",
                txid_bytes.len()
            )));
        }

        let mut txid_arr = [0u8; 64];
        txid_arr.copy_from_slice(&txid_bytes);

        Ok(OutPoint::new(Hash::from_bytes(txid_arr), self.index))
    }
}

/// Serializable locking condition data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LockingConditionData {
    /// Pay to Public Key Hash.
    P2PKH {
        /// The address (hex-encoded).
        address: String,
        /// The public key (hex-encoded, if known).
        public_key: Option<String>,
    },
    /// M-of-N Multisig.
    Multisig {
        /// Number of required signatures.
        threshold: u8,
        /// The public keys (hex-encoded).
        public_keys: Vec<String>,
    },
}

impl LockingConditionData {
    /// Create from a P2PKH address and public key.
    pub fn p2pkh(address: &Address, public_key: Option<&PublicKey>) -> Self {
        Self::P2PKH {
            address: address.to_hex(),
            public_key: public_key.map(|pk| hex::encode(pk.to_bytes())),
        }
    }

    /// Create from a multisig condition.
    pub fn multisig(threshold: u8, public_keys: &[PublicKey]) -> Self {
        Self::Multisig {
            threshold,
            public_keys: public_keys
                .iter()
                .map(|pk| hex::encode(pk.to_bytes()))
                .collect(),
        }
    }

    /// Get the public keys for a multisig condition.
    pub fn get_multisig_keys(&self) -> Result<Vec<PublicKey>, WalletError> {
        match self {
            Self::Multisig { public_keys, .. } => {
                let mut keys = Vec::with_capacity(public_keys.len());
                for pk_hex in public_keys {
                    let pk_bytes = hex::decode(pk_hex).map_err(|e| {
                        WalletError::InvalidFormat(format!("invalid public key hex: {e}"))
                    })?;
                    let pk = PublicKey::from_bytes(&pk_bytes)
                        .ok_or_else(|| WalletError::InvalidFormat("invalid public key".into()))?;
                    keys.push(pk);
                }
                Ok(keys)
            }
            Self::P2PKH { .. } => Err(WalletError::InvalidFormat(
                "not a multisig condition".into(),
            )),
        }
    }

    /// Get the threshold for a multisig condition.
    pub fn get_threshold(&self) -> Result<u8, WalletError> {
        match self {
            Self::Multisig { threshold, .. } => Ok(*threshold),
            Self::P2PKH { .. } => Err(WalletError::InvalidFormat(
                "not a multisig condition".into(),
            )),
        }
    }

    /// Convert to a LockingCondition.
    pub fn to_locking_condition(&self) -> Result<LockingCondition, WalletError> {
        match self {
            Self::P2PKH { address, .. } => {
                let addr_bytes = hex::decode(address)
                    .map_err(|e| WalletError::InvalidFormat(format!("invalid address hex: {e}")))?;
                if addr_bytes.len() != 64 {
                    return Err(WalletError::InvalidFormat(format!(
                        "address must be 64 bytes, got {}",
                        addr_bytes.len()
                    )));
                }
                let mut addr_arr = [0u8; 64];
                addr_arr.copy_from_slice(&addr_bytes);
                Ok(LockingCondition::P2PKH(Address::from_hash(
                    Hash::from_bytes(addr_arr),
                )))
            }
            Self::Multisig {
                threshold,
                public_keys: _,
            } => {
                let keys = self.get_multisig_keys()?;
                Ok(LockingCondition::multisig(*threshold, keys))
            }
        }
    }
}

/// A transaction output specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartialOutput {
    /// Amount in quanta.
    pub amount: u64,
    /// The locking condition.
    pub condition: LockingConditionData,
}

impl PartialOutput {
    /// Create a P2PKH output.
    pub fn p2pkh(amount: u64, address: &Address) -> Self {
        Self {
            amount,
            condition: LockingConditionData::P2PKH {
                address: address.to_hex(),
                public_key: None,
            },
        }
    }

    /// Create a multisig output.
    pub fn multisig(amount: u64, threshold: u8, public_keys: &[PublicKey]) -> Self {
        Self {
            amount,
            condition: LockingConditionData::multisig(threshold, public_keys),
        }
    }

    /// Convert to a TxOutput.
    pub fn to_tx_output(&self) -> Result<TxOutput, WalletError> {
        let condition = self.condition.to_locking_condition()?;
        Ok(TxOutput {
            amount: self.amount,
            condition,
        })
    }
}

impl PartialTransaction {
    /// Create a new empty partial transaction.
    pub fn new() -> Self {
        Self {
            version: Transaction::CURRENT_VERSION,
            inputs: Vec::new(),
            outputs: Vec::new(),
            fee: 0,
        }
    }

    /// Add an input to the transaction.
    pub fn add_input(&mut self, input: PartialInput) {
        self.inputs.push(input);
    }

    /// Add an output to the transaction.
    pub fn add_output(&mut self, output: PartialOutput) {
        self.outputs.push(output);
    }

    /// Set the transaction fee.
    pub fn set_fee(&mut self, fee: u64) {
        self.fee = fee;
    }

    /// Add a signature to a multisig input.
    ///
    /// # Arguments
    ///
    /// * `input_index` - The index of the input to sign.
    /// * `key_index` - The index of the public key in the multisig set.
    /// * `signature` - The signature.
    pub fn add_signature(
        &mut self,
        input_index: usize,
        key_index: usize,
        signature: &Signature,
    ) -> Result<(), WalletError> {
        let input = self
            .inputs
            .get_mut(input_index)
            .ok_or_else(|| WalletError::InvalidFormat("input index out of range".into()))?;

        if key_index >= input.signatures.len() {
            return Err(WalletError::InvalidFormat("key index out of range".into()));
        }

        input.signatures[key_index] = Some(hex::encode(signature.to_bytes()));
        Ok(())
    }

    /// Check if a specific input has enough signatures.
    pub fn input_is_complete(&self, input_index: usize) -> Result<bool, WalletError> {
        let input = self
            .inputs
            .get(input_index)
            .ok_or_else(|| WalletError::InvalidFormat("input index out of range".into()))?;

        let required = match &input.locking_condition {
            LockingConditionData::P2PKH { .. } => 1,
            LockingConditionData::Multisig { threshold, .. } => *threshold as usize,
        };

        let signed = input.signatures.iter().filter(|s| s.is_some()).count();
        Ok(signed >= required)
    }

    /// Check if all inputs have enough signatures.
    pub fn is_complete(&self) -> bool {
        for i in 0..self.inputs.len() {
            match self.input_is_complete(i) {
                Ok(true) => continue,
                _ => return false,
            }
        }
        true
    }

    /// Count the number of signatures for an input.
    pub fn signature_count(&self, input_index: usize) -> Result<(usize, usize), WalletError> {
        let input = self
            .inputs
            .get(input_index)
            .ok_or_else(|| WalletError::InvalidFormat("input index out of range".into()))?;

        let required = match &input.locking_condition {
            LockingConditionData::P2PKH { .. } => 1,
            LockingConditionData::Multisig { threshold, .. } => *threshold as usize,
        };

        let signed = input.signatures.iter().filter(|s| s.is_some()).count();
        Ok((signed, required))
    }

    /// Build a complete Transaction from this partial transaction.
    ///
    /// Returns an error if not all inputs have sufficient signatures.
    pub fn to_transaction(&self) -> Result<Transaction, WalletError> {
        if !self.is_complete() {
            return Err(WalletError::InvalidFormat(
                "not all inputs have sufficient signatures".into(),
            ));
        }

        let mut inputs = Vec::with_capacity(self.inputs.len());
        for partial_input in &self.inputs {
            let outpoint = partial_input.outpoint.to_outpoint()?;
            let witness = self.build_witness(partial_input)?;
            inputs.push(TxInput::new(outpoint, witness));
        }

        let mut outputs = Vec::with_capacity(self.outputs.len());
        for partial_output in &self.outputs {
            outputs.push(partial_output.to_tx_output()?);
        }

        Ok(Transaction {
            version: self.version,
            inputs,
            outputs,
        })
    }

    /// Build a witness from a partial input.
    fn build_witness(&self, input: &PartialInput) -> Result<Witness, WalletError> {
        match &input.locking_condition {
            LockingConditionData::P2PKH { public_key, .. } => {
                let pk_hex = public_key
                    .as_ref()
                    .ok_or_else(|| WalletError::InvalidFormat("P2PKH missing public key".into()))?;
                let pk_bytes = hex::decode(pk_hex).map_err(|e| {
                    WalletError::InvalidFormat(format!("invalid public key hex: {e}"))
                })?;
                let public_key = PublicKey::from_bytes(&pk_bytes)
                    .ok_or_else(|| WalletError::InvalidFormat("invalid public key".into()))?;

                let sig_hex = input.signatures[0]
                    .as_ref()
                    .ok_or_else(|| WalletError::InvalidFormat("P2PKH missing signature".into()))?;
                let sig_bytes = hex::decode(sig_hex).map_err(|e| {
                    WalletError::InvalidFormat(format!("invalid signature hex: {e}"))
                })?;
                let signature = Signature::from_bytes(&sig_bytes)
                    .ok_or_else(|| WalletError::InvalidFormat("invalid signature".into()))?;

                Ok(Witness::P2PKH {
                    public_key: Box::new(public_key),
                    signature: Box::new(signature),
                })
            }
            LockingConditionData::Multisig { public_keys, .. } => {
                let mut keys = Vec::with_capacity(public_keys.len());
                for pk_hex in public_keys {
                    let pk_bytes = hex::decode(pk_hex).map_err(|e| {
                        WalletError::InvalidFormat(format!("invalid public key hex: {e}"))
                    })?;
                    let pk = PublicKey::from_bytes(&pk_bytes)
                        .ok_or_else(|| WalletError::InvalidFormat("invalid public key".into()))?;
                    keys.push(pk);
                }

                let mut signatures = Vec::with_capacity(input.signatures.len());
                for sig_opt in &input.signatures {
                    if let Some(sig_hex) = sig_opt {
                        let sig_bytes = hex::decode(sig_hex).map_err(|e| {
                            WalletError::InvalidFormat(format!("invalid signature hex: {e}"))
                        })?;
                        let sig = Signature::from_bytes(&sig_bytes).ok_or_else(|| {
                            WalletError::InvalidFormat("invalid signature".into())
                        })?;
                        signatures.push(Some(sig));
                    } else {
                        signatures.push(None);
                    }
                }

                Ok(Witness::Multisig {
                    public_keys: keys,
                    signatures,
                })
            }
        }
    }

    /// Save the partial transaction to a file.
    pub fn save(&self, path: &Path) -> Result<(), WalletError> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| WalletError::InvalidFormat(format!("serialization failed: {e}")))?;

        std::fs::write(path, json).map_err(|e| WalletError::Io(format!("write failed: {e}")))?;

        Ok(())
    }

    /// Load a partial transaction from a file.
    pub fn load(path: &Path) -> Result<Self, WalletError> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| WalletError::Io(format!("read failed: {e}")))?;

        let partial: Self = serde_json::from_str(&json)
            .map_err(|e| WalletError::InvalidFormat(format!("parse failed: {e}")))?;

        Ok(partial)
    }

    /// Get the signing data for an input.
    ///
    /// This builds a temporary transaction to compute the signing data.
    pub fn signing_data(&self, input_index: usize) -> Result<Vec<u8>, WalletError> {
        // Build inputs with placeholder witnesses
        let mut inputs = Vec::with_capacity(self.inputs.len());
        for partial_input in &self.inputs {
            let outpoint = partial_input.outpoint.to_outpoint()?;
            // Use a placeholder witness for signing
            let witness = Witness::Coinbase(vec![]);
            inputs.push(TxInput::new(outpoint, witness));
        }

        // Build outputs
        let mut outputs = Vec::with_capacity(self.outputs.len());
        for partial_output in &self.outputs {
            outputs.push(partial_output.to_tx_output()?);
        }

        let tx = Transaction {
            version: self.version,
            inputs,
            outputs,
        };

        Ok(tx.signing_data(input_index))
    }

    /// Sign an input with a keypair and return the key index if successful.
    ///
    /// Returns the key index if the keypair's public key is part of the multisig,
    /// or an error if not.
    pub fn sign_input(
        &mut self,
        input_index: usize,
        public_key: &PublicKey,
        secret_key: &crate::crypto::SecretKey,
    ) -> Result<usize, WalletError> {
        let input = self
            .inputs
            .get(input_index)
            .ok_or_else(|| WalletError::InvalidFormat("input index out of range".into()))?;

        // Find the key index
        let key_index = match &input.locking_condition {
            LockingConditionData::P2PKH {
                public_key: _pk_opt,
                address,
            } => {
                // For P2PKH, verify the public key matches the address
                let pk_hash = crypto::hash(public_key.as_ref());
                let addr_bytes = hex::decode(address)
                    .map_err(|e| WalletError::InvalidFormat(format!("invalid address hex: {e}")))?;
                if pk_hash.as_bytes() != addr_bytes.as_slice() {
                    return Err(WalletError::InvalidFormat(
                        "public key does not match address".into(),
                    ));
                }
                // Update the public key in the input
                let input = self.inputs.get_mut(input_index).unwrap();
                if let LockingConditionData::P2PKH {
                    public_key: pk_opt, ..
                } = &mut input.locking_condition
                {
                    *pk_opt = Some(hex::encode(public_key.to_bytes()));
                }
                0
            }
            LockingConditionData::Multisig { public_keys, .. } => {
                let pk_hex = hex::encode(public_key.to_bytes());
                public_keys
                    .iter()
                    .position(|pk| pk == &pk_hex)
                    .ok_or_else(|| {
                        WalletError::InvalidFormat("public key not found in multisig set".into())
                    })?
            }
        };

        // Get signing data and sign
        let signing_data = self.signing_data(input_index)?;
        let message_hash = crypto::hash(&signing_data);
        let signature = crate::crypto::ml_dsa_87::sign(secret_key, message_hash.as_bytes());

        // Add the signature
        self.add_signature(input_index, key_index, &signature)?;

        Ok(key_index)
    }

    /// Merge signatures from another partial transaction.
    ///
    /// This is used to combine partial transactions that have been signed
    /// by different parties.
    pub fn merge(&mut self, other: &PartialTransaction) -> Result<(), WalletError> {
        if self.inputs.len() != other.inputs.len() {
            return Err(WalletError::InvalidFormat("input count mismatch".into()));
        }

        for (i, (self_input, other_input)) in
            self.inputs.iter_mut().zip(other.inputs.iter()).enumerate()
        {
            if self_input.signatures.len() != other_input.signatures.len() {
                return Err(WalletError::InvalidFormat(format!(
                    "signature slot count mismatch for input {i}"
                )));
            }

            for (j, (self_sig, other_sig)) in self_input
                .signatures
                .iter_mut()
                .zip(other_input.signatures.iter())
                .enumerate()
            {
                if self_sig.is_none() && other_sig.is_some() {
                    *self_sig = other_sig.clone();
                } else if self_sig.is_some() && other_sig.is_some() && self_sig != other_sig {
                    return Err(WalletError::InvalidFormat(format!(
                        "conflicting signatures for input {i}, key {j}"
                    )));
                }
            }
        }

        Ok(())
    }
}

impl Default for PartialTransaction {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::KeyPair;

    #[test]
    fn test_partial_transaction_p2pkh_roundtrip() {
        let keypair = KeyPair::generate();
        let address = keypair.address();

        let mut partial = PartialTransaction::new();
        partial.add_input(PartialInput {
            outpoint: OutPointData {
                txid: "00".repeat(64),
                index: 0,
            },
            amount: 10000,
            locking_condition: LockingConditionData::P2PKH {
                address: address.to_hex(),
                public_key: None,
            },
            signatures: vec![None],
        });
        partial.add_output(PartialOutput::p2pkh(9000, &address));
        partial.set_fee(1000);

        // Sign the input
        partial
            .sign_input(0, keypair.public_key(), keypair.secret_key())
            .unwrap();

        assert!(partial.is_complete());

        // Convert to transaction
        let tx = partial.to_transaction().unwrap();
        assert_eq!(tx.inputs.len(), 1);
        assert_eq!(tx.outputs.len(), 1);
    }

    #[test]
    fn test_partial_transaction_multisig() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();
        let carol = KeyPair::generate();

        let public_keys = vec![
            alice.public_key().clone(),
            bob.public_key().clone(),
            carol.public_key().clone(),
        ];

        let mut partial = PartialTransaction::new();
        partial.add_input(PartialInput {
            outpoint: OutPointData {
                txid: "00".repeat(64),
                index: 0,
            },
            amount: 10000,
            locking_condition: LockingConditionData::multisig(2, &public_keys),
            signatures: vec![None, None, None],
        });
        partial.add_output(PartialOutput::p2pkh(9000, &alice.address()));
        partial.set_fee(1000);

        // Not complete yet
        assert!(!partial.is_complete());
        assert_eq!(partial.signature_count(0).unwrap(), (0, 2));

        // Sign with Alice
        partial
            .sign_input(0, alice.public_key(), alice.secret_key())
            .unwrap();
        assert!(!partial.is_complete());
        assert_eq!(partial.signature_count(0).unwrap(), (1, 2));

        // Sign with Bob
        partial
            .sign_input(0, bob.public_key(), bob.secret_key())
            .unwrap();
        assert!(partial.is_complete());
        assert_eq!(partial.signature_count(0).unwrap(), (2, 2));

        // Convert to transaction
        let tx = partial.to_transaction().unwrap();
        assert_eq!(tx.inputs.len(), 1);
        assert!(matches!(tx.inputs[0].witness, Witness::Multisig { .. }));
    }

    #[test]
    fn test_partial_transaction_file_roundtrip() {
        let keypair = KeyPair::generate();
        let address = keypair.address();

        let mut partial = PartialTransaction::new();
        partial.add_input(PartialInput {
            outpoint: OutPointData {
                txid: "01".repeat(64),
                index: 1,
            },
            amount: 5000,
            locking_condition: LockingConditionData::P2PKH {
                address: address.to_hex(),
                public_key: Some(hex::encode(keypair.public_key().to_bytes())),
            },
            signatures: vec![None],
        });
        partial.add_output(PartialOutput::p2pkh(4000, &address));
        partial.set_fee(1000);

        // Save and load
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.tx");
        partial.save(&path).unwrap();

        let loaded = PartialTransaction::load(&path).unwrap();
        assert_eq!(loaded.inputs.len(), 1);
        assert_eq!(loaded.outputs.len(), 1);
        assert_eq!(loaded.fee, 1000);
    }

    #[test]
    fn test_partial_transaction_merge() {
        let alice = KeyPair::generate();
        let bob = KeyPair::generate();

        let public_keys = vec![alice.public_key().clone(), bob.public_key().clone()];

        // Create base transaction
        let create_base = || {
            let mut partial = PartialTransaction::new();
            partial.add_input(PartialInput {
                outpoint: OutPointData {
                    txid: "00".repeat(64),
                    index: 0,
                },
                amount: 10000,
                locking_condition: LockingConditionData::multisig(2, &public_keys),
                signatures: vec![None, None],
            });
            partial.add_output(PartialOutput::p2pkh(9000, &alice.address()));
            partial.set_fee(1000);
            partial
        };

        // Alice signs her copy
        let mut alice_partial = create_base();
        alice_partial
            .sign_input(0, alice.public_key(), alice.secret_key())
            .unwrap();

        // Bob signs his copy
        let mut bob_partial = create_base();
        bob_partial
            .sign_input(0, bob.public_key(), bob.secret_key())
            .unwrap();

        // Merge Bob's signatures into Alice's copy
        alice_partial.merge(&bob_partial).unwrap();

        // Should now be complete
        assert!(alice_partial.is_complete());
    }
}
