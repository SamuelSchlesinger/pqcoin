//! Transaction validation for mempool inclusion.

use crate::blockchain::{Blockchain, LockingCondition, Transaction, TxOutput, Witness};
use crate::crypto::{hash, ml_dsa_87};

use super::Mempool;
use super::error::MempoolError;

/// Validate a transaction for mempool inclusion.
pub(crate) fn validate_mempool_tx(
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
pub(crate) fn validate_witness(
    witness: &Witness,
    output: &TxOutput,
    tx: &Transaction,
    input_index: usize,
) -> bool {
    match (&output.condition, witness) {
        (
            LockingCondition::P2PKH(addr),
            Witness::P2PKH {
                public_key,
                signature,
            },
        ) => {
            // Check public key hashes to address
            let pk_hash = hash((**public_key).as_ref());
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
            LockingCondition::Multisig {
                threshold,
                public_keys,
            },
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
