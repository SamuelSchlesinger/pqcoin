//! Transaction verification logic.

use crate::constants::COINBASE_MATURITY;
use crate::crypto::{self, Hash};
use rayon::prelude::*;
use std::collections::HashMap;

use super::address::Address;
use super::error::BlockchainError;
use super::locking::LockingCondition;
use super::outpoint::OutPoint;
use super::transaction::Transaction;
use super::utxo::Utxo;
use super::witness::Witness;

/// Verify signatures for a batch of transactions in parallel.
///
/// Uses rayon to verify multiple transaction signatures concurrently for better performance.
pub(crate) fn verify_transactions_parallel(
    transactions: &[Transaction],
    height: u64,
    utxos: &HashMap<OutPoint, Utxo>,
) -> Result<Vec<u64>, BlockchainError> {
    // Collect verification data for all transactions first
    let verify_data: Vec<_> = transactions
        .iter()
        .map(|tx| {
            let mut input_data = Vec::new();
            for (input_index, input) in tx.inputs.iter().enumerate() {
                let utxo = utxos.get(&input.outpoint);
                let signing_data = tx.signing_data(input_index);
                let message = crypto::hash(&signing_data);
                input_data.push((input_index, utxo.cloned(), message, input.witness.clone()));
            }
            (tx, input_data)
        })
        .collect();

    // Verify all signatures in parallel
    let results: Vec<Result<u64, BlockchainError>> = verify_data
        .par_iter()
        .map(|(tx, input_data)| {
            if tx.is_coinbase() {
                return Ok(0);
            }

            let mut input_sum = 0u64;

            for (input_index, utxo_opt, message, witness) in input_data {
                let utxo = utxo_opt.as_ref().ok_or(BlockchainError::MissingInput(
                    tx.inputs[*input_index].outpoint,
                ))?;

                // Coinbase outputs need maturity before they can be spent
                if utxo.is_coinbase && height < utxo.height + COINBASE_MATURITY {
                    return Err(BlockchainError::ImmatureCoinbase {
                        outpoint: tx.inputs[*input_index].outpoint,
                        current_height: height,
                        maturity_height: utxo.height + COINBASE_MATURITY,
                    });
                }

                input_sum = input_sum
                    .checked_add(utxo.output.amount)
                    .ok_or(BlockchainError::InsufficientInputs)?;

                // Verify the witness matches the locking condition
                verify_witness(&utxo.output.condition, witness, message)?;
            }

            Ok(input_sum)
        })
        .collect();

    // Collect results, returning first error if any
    results.into_iter().collect()
}

/// Verify a transaction's signatures (single-threaded version).
///
/// # Signature Verification Design
///
/// Before passing data to ML-DSA-87, we hash the signing data with SHA3-512.
/// This is intentional and correct for two reasons:
///
/// 1. **ML-DSA-87 signs raw messages**: Unlike some signature schemes, ML-DSA-87
///    (FIPS 204) does not perform internal hashing - it signs the message directly.
///    Pre-hashing is appropriate for potentially large transaction data.
///
/// 2. **Fixed-size input**: Hashing produces a fixed 64-byte input regardless of
///    transaction size, which is more efficient for the signature algorithm.
///
/// This matches Bitcoin's approach of signing the SHA256d hash of transaction data.
#[allow(dead_code)]
pub(crate) fn verify_transaction(
    tx: &Transaction,
    height: u64,
    utxos: &HashMap<OutPoint, Utxo>,
) -> Result<u64, BlockchainError> {
    if tx.is_coinbase() {
        // Coinbase transactions are verified differently
        return Ok(0);
    }

    let mut input_sum = 0u64;

    for (input_index, input) in tx.inputs.iter().enumerate() {
        // Get the UTXO being spent
        let utxo = utxos
            .get(&input.outpoint)
            .ok_or(BlockchainError::MissingInput(input.outpoint))?;

        // Coinbase outputs need maturity before they can be spent
        if utxo.is_coinbase && height < utxo.height + COINBASE_MATURITY {
            return Err(BlockchainError::ImmatureCoinbase {
                outpoint: input.outpoint,
                current_height: height,
                maturity_height: utxo.height + COINBASE_MATURITY,
            });
        }

        input_sum = input_sum
            .checked_add(utxo.output.amount)
            .ok_or(BlockchainError::InsufficientInputs)?;

        // Verify the witness matches the locking condition
        let signing_data = tx.signing_data(input_index);
        let message = crypto::hash(&signing_data);

        verify_witness(&utxo.output.condition, &input.witness, &message)?;
    }

    Ok(input_sum)
}

/// Verify that a witness satisfies a locking condition.
///
/// # Security Properties
///
/// This function is critical for consensus security. It verifies that a transaction
/// input is authorized to spend the referenced UTXO.
///
/// ## ML-DSA-87 Verification
///
/// SECURITY: The `crypto::ml_dsa_87::verify()` function uses the `pqcrypto-dilithium`
/// crate which implements FIPS 204 (ML-DSA). The verification operation:
///
/// - Is constant-time with respect to the secret key (not applicable here, but the
///   implementation is consistent)
/// - May have variable timing based on the public key and message, which is acceptable
///   as these are public values
/// - Returns a boolean result, not distinguishing between different failure modes
///
/// ## Address Binding
///
/// SECURITY: The public key is hashed with SHA3-512 to derive the address. An attacker
/// cannot substitute a different public key that hashes to the same address due to
/// SHA3-512's collision resistance (256-bit security against collision attacks).
fn verify_witness(
    condition: &LockingCondition,
    witness: &Witness,
    message: &Hash,
) -> Result<(), BlockchainError> {
    match (condition, witness) {
        (
            LockingCondition::P2PKH(address),
            Witness::P2PKH {
                public_key,
                signature,
            },
        ) => {
            // SECURITY: Verify public key binds to address before checking signature.
            // This prevents signature malleability attacks where an attacker might
            // provide a valid signature from a different key.
            if Address::from_public_key(public_key) != *address {
                return Err(BlockchainError::InvalidWitness);
            }
            // SECURITY: ML-DSA-87 signature verification. The message is already
            // hashed (SHA3-512 of signing data), providing domain separation.
            if !crypto::ml_dsa_87::verify(public_key, message.as_bytes(), signature) {
                return Err(BlockchainError::InvalidWitness);
            }
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
                return Err(BlockchainError::InvalidWitness);
            }
            for (wk, pk) in witness_keys.iter().zip(public_keys.iter()) {
                if wk.to_bytes() != pk.to_bytes() {
                    return Err(BlockchainError::InvalidWitness);
                }
            }
            // Count valid signatures
            let mut valid_sigs = 0u8;
            for (i, sig_opt) in signatures.iter().enumerate() {
                if let Some(sig) = sig_opt {
                    if i < public_keys.len()
                        && crypto::ml_dsa_87::verify(&public_keys[i], message.as_bytes(), sig)
                    {
                        valid_sigs += 1;
                    }
                }
            }
            if valid_sigs < *threshold {
                return Err(BlockchainError::InvalidWitness);
            }
        }
        _ => return Err(BlockchainError::InvalidWitness),
    }

    Ok(())
}
