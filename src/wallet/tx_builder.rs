//! Transaction builder for pqcoin wallets.

use crate::blockchain::{Address, OutPoint, Transaction, TxInput, TxOutput, Witness};
use crate::crypto::hash;

use super::keys::{KeyPair, WalletError};

/// UTXO information for building transactions.
#[derive(Debug, Clone)]
pub struct UtxoInput {
    /// The outpoint referencing this UTXO.
    pub outpoint: OutPoint,
    /// The amount in quanta.
    pub amount: u64,
    /// Block height where this UTXO was created.
    pub height: u64,
    /// Whether this is a coinbase output.
    pub is_coinbase: bool,
}

/// Transaction output specification.
#[derive(Debug, Clone)]
pub struct TxOutputSpec {
    /// Recipient address.
    pub address: Address,
    /// Amount in quanta.
    pub amount: u64,
}

/// Builder for constructing and signing transactions.
pub struct TransactionBuilder {
    /// Available UTXOs for spending.
    utxos: Vec<UtxoInput>,
    /// Outputs to create.
    outputs: Vec<TxOutputSpec>,
    /// Change address.
    change_address: Option<Address>,
    /// Fee rate (quanta per byte, simplified to flat fee).
    fee: u64,
    /// Current blockchain height (for coinbase maturity checks).
    current_height: u64,
}

impl TransactionBuilder {
    /// Create a new transaction builder.
    pub fn new() -> Self {
        Self {
            utxos: Vec::new(),
            outputs: Vec::new(),
            change_address: None,
            fee: 1000, // Default fee: 1000 quanta
            current_height: 0,
        }
    }

    /// Set available UTXOs.
    pub fn with_utxos(mut self, utxos: Vec<UtxoInput>) -> Self {
        self.utxos = utxos;
        self
    }

    /// Add an output.
    pub fn add_output(mut self, address: Address, amount: u64) -> Self {
        self.outputs.push(TxOutputSpec { address, amount });
        self
    }

    /// Set the change address.
    pub fn with_change_address(mut self, address: Address) -> Self {
        self.change_address = Some(address);
        self
    }

    /// Set the transaction fee.
    pub fn with_fee(mut self, fee: u64) -> Self {
        self.fee = fee;
        self
    }

    /// Set the current blockchain height (for coinbase maturity).
    pub fn with_current_height(mut self, height: u64) -> Self {
        self.current_height = height;
        self
    }

    /// Build and sign the transaction.
    pub fn build(self, keypair: &KeyPair) -> Result<Transaction, WalletError> {
        // Calculate total output amount
        let total_output: u64 = self.outputs.iter().map(|o| o.amount).sum();
        let total_needed = total_output + self.fee;

        // Select UTXOs (simple greedy algorithm)
        let spendable_utxos: Vec<&UtxoInput> = self
            .utxos
            .iter()
            .filter(|u| {
                // Check coinbase maturity
                if u.is_coinbase {
                    self.current_height >= u.height + crate::constants::COINBASE_MATURITY
                } else {
                    true
                }
            })
            .collect();

        let mut selected: Vec<&UtxoInput> = Vec::new();
        let mut total_selected: u64 = 0;

        for utxo in spendable_utxos {
            if total_selected >= total_needed {
                break;
            }
            selected.push(utxo);
            total_selected += utxo.amount;
        }

        if total_selected < total_needed {
            return Err(WalletError::Crypto(format!(
                "insufficient funds: have {}, need {} (including {} fee)",
                total_selected, total_needed, self.fee
            )));
        }

        // Build inputs (with placeholder signatures)
        let inputs: Vec<TxInput> = selected
            .iter()
            .map(|utxo| {
                TxInput::new(
                    utxo.outpoint,
                    Witness::P2PKH {
                        public_key: Box::new(keypair.public_key().clone()),
                        signature: Box::new(crate::crypto::ml_dsa_87::sign(
                            keypair.secret_key(),
                            &[0u8; 64], // Placeholder
                        )),
                    },
                )
            })
            .collect();

        // Build outputs
        let mut outputs: Vec<TxOutput> = self
            .outputs
            .iter()
            .map(|o| TxOutput::p2pkh(o.amount, o.address))
            .collect();

        // Add change output if needed
        let change = total_selected - total_needed;
        if change > 0 {
            let change_address = self.change_address.unwrap_or_else(|| keypair.address());
            outputs.push(TxOutput::p2pkh(change, change_address));
        }

        // Create the transaction
        let mut tx = Transaction::new(inputs, outputs);

        // Sign each input
        for (i, _utxo) in selected.iter().enumerate() {
            let signing_data = tx.signing_data(i);
            let message_hash = hash(&signing_data);
            let signature =
                crate::crypto::ml_dsa_87::sign(keypair.secret_key(), message_hash.as_bytes());

            tx.inputs[i].witness = Witness::P2PKH {
                public_key: Box::new(keypair.public_key().clone()),
                signature: Box::new(signature),
            };
        }

        Ok(tx)
    }

    /// Estimate the fee for a transaction with the given parameters.
    pub fn estimate_fee(&self) -> u64 {
        // Simple flat fee for now
        // In a real implementation, this would be based on transaction size
        self.fee
    }
}

impl Default for TransactionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Hash;

    #[test]
    fn test_transaction_builder_insufficient_funds() {
        let keypair = KeyPair::generate();

        let result = TransactionBuilder::new()
            .with_utxos(vec![UtxoInput {
                outpoint: OutPoint::new(Hash::from_bytes([0u8; 64]), 0),
                amount: 1000,
                height: 0,
                is_coinbase: false,
            }])
            .add_output(keypair.address(), 2000)
            .build(&keypair);

        assert!(result.is_err());
    }

    #[test]
    fn test_transaction_builder_with_change() {
        let keypair = KeyPair::generate();
        let recipient = KeyPair::generate().address();

        let tx = TransactionBuilder::new()
            .with_utxos(vec![UtxoInput {
                outpoint: OutPoint::new(Hash::from_bytes([1u8; 64]), 0),
                amount: 10000,
                height: 0,
                is_coinbase: false,
            }])
            .add_output(recipient, 5000)
            .with_fee(1000)
            .build(&keypair)
            .unwrap();

        // Should have 2 outputs: payment + change
        assert_eq!(tx.outputs.len(), 2);

        // Check amounts
        let total_out: u64 = tx.outputs.iter().map(|o| o.amount).sum();
        assert_eq!(total_out, 9000); // 10000 - 1000 fee
    }
}
