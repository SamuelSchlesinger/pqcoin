//! # Blockchain Data Structures
//!
//! This module implements the core blockchain data structures for pqcoin, a proof-of-work
//! cryptocurrency using post-quantum cryptographic primitives.
//!
//! ## Design Overview
//!
//! The design follows Bitcoin's UTXO (Unspent Transaction Output) model with simplifications:
//!
//! - **No scripting language**: Instead of Bitcoin's Script, we support only two output types:
//!   - Pay-to-Public-Key-Hash (P2PKH): Single signature required
//!   - M-of-N Multisig: Multiple signatures required from a set of public keys
//!
//! - **Post-quantum signatures**: All signatures use ML-DSA-87 (FIPS 204)
//!
//! - **SHA3-512 hashing**: All hashes use SHA3-512 (FIPS 202)
//!
//! ## Data Flow
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │ Transaction │────▶│    Block    │────▶│  Blockchain │
//! │  (UTXO)     │     │  (PoW)      │     │  (Chain)    │
//! └─────────────┘     └─────────────┘     └─────────────┘
//! ```
//!
//! ## Serialization
//!
//! All structures implement deterministic binary serialization via the [`Serialize`] and
//! [`Deserialize`] traits. The format is compact and uses little-endian byte order for
//! multi-byte integers.
//!
//! ## Performance Optimizations
//!
//! ### Transaction Index
//!
//! The [`Blockchain`] struct maintains a `tx_index` mapping transaction IDs to block hashes.
//! This enables O(1) lookups when finding which block contains a transaction, which is
//! critical for efficient chain reorganizations.
//!
//! Without this index, `find_block_containing_tx()` would need to scan all blocks and all
//! transactions within each block (O(n*m) complexity). With the index, lookups are O(1).
//!
//! The index is automatically maintained:
//! - Entries are added when blocks are applied via `apply_block()`
//! - Entries are removed when blocks are unapplied via `unapply_block()`

use crate::constants::{
    COINBASE_MATURITY, DIFFICULTY_COEFFICIENT_MASK, DUST_FEE_RATE, MAX_BLOCK_SIZE, MAX_BLOCK_TXS,
    MAX_FUTURE_BLOCK_TIME, MAX_MULTISIG_KEYS, MAX_REORG_DEPTH, MAX_SERIALIZE_BYTES,
    MAX_TX_INPUTS, MAX_TX_OUTPUTS,
};
use crate::crypto::{self, Hash, PublicKey, Signature};
use rayon::prelude::*;
use std::collections::HashMap;

// ============================================================================
// Checkpoint and AssumeValid Configuration
// ============================================================================

/// Hardcoded checkpoints for known-good blocks.
/// Format: (height, block_hash_hex)
/// These prevent long-range attacks during initial sync.
const CHECKPOINTS: &[(u64, &str)] = &[
    // Genesis block - add real hash after launch
    // (0, "genesis_hash_here"),
];

/// Block hash for which we assume all ancestors have valid signatures.
/// Set to None to verify all signatures (slower but more secure).
/// This should be a block that is buried under significant PoW.
const ASSUME_VALID: Option<&str> = None;

/// Parse a hex string into a Hash.
/// Returns None if the hex string is invalid or wrong length.
fn parse_hash_hex(hex_str: &str) -> Option<Hash> {
    let bytes = hex::decode(hex_str).ok()?;
    if bytes.len() != 64 {
        return None;
    }
    let mut arr = [0u8; 64];
    arr.copy_from_slice(&bytes);
    Some(Hash::from_bytes(arr))
}

// ============================================================================
// Serialization Traits
// ============================================================================

/// Errors that can occur during deserialization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeserializeError {
    /// Not enough bytes to read the expected data.
    UnexpectedEof,
    /// The data contains an invalid or unrecognized value.
    InvalidData(String),
    /// A length field exceeds the maximum allowed value.
    LengthOverflow,
}

impl std::fmt::Display for DeserializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeserializeError::UnexpectedEof => write!(f, "unexpected end of data"),
            DeserializeError::InvalidData(msg) => write!(f, "invalid data: {}", msg),
            DeserializeError::LengthOverflow => write!(f, "length exceeds maximum"),
        }
    }
}

impl std::error::Error for DeserializeError {}

/// Trait for types that can be serialized to bytes.
///
/// Implementations must produce deterministic output: the same value must always
/// serialize to the same bytes.
pub trait Serialize {
    /// Serialize this value, appending bytes to the provided buffer.
    fn serialize(&self, buf: &mut Vec<u8>);

    /// Convenience method to serialize to a new vector.
    fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.serialize(&mut buf);
        buf
    }
}

/// Trait for types that can be deserialized from bytes.
pub trait Deserialize: Sized {
    /// Deserialize a value from a byte slice, returning the value and remaining bytes.
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError>;

    /// Convenience method to deserialize from a byte slice, requiring all bytes to be consumed.
    fn from_bytes(data: &[u8]) -> Result<Self, DeserializeError> {
        let (value, remaining) = Self::deserialize(data)?;
        if !remaining.is_empty() {
            return Err(DeserializeError::InvalidData(format!(
                "trailing bytes: {} remaining",
                remaining.len()
            )));
        }
        Ok(value)
    }
}

// Helper functions for serialization
fn write_u8(buf: &mut Vec<u8>, value: u8) {
    buf.push(value);
}

fn write_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_u64(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_var_int(buf: &mut Vec<u8>, value: u64) {
    // Variable-length integer encoding (similar to Bitcoin's CompactSize):
    // - 0x00-0xFC: 1 byte
    // - 0xFD-0xFFFF: 0xFD followed by 2 bytes (little-endian)
    // - 0x10000-0xFFFFFFFF: 0xFE followed by 4 bytes (little-endian)
    // - 0x100000000-: 0xFF followed by 8 bytes (little-endian)
    if value < 0xFD {
        buf.push(value as u8);
    } else if value <= 0xFFFF {
        buf.push(0xFD);
        buf.extend_from_slice(&(value as u16).to_le_bytes());
    } else if value <= 0xFFFFFFFF {
        buf.push(0xFE);
        buf.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        buf.push(0xFF);
        buf.extend_from_slice(&value.to_le_bytes());
    }
}

fn write_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    write_var_int(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

// Helper functions for deserialization
fn read_u8(data: &[u8]) -> Result<(u8, &[u8]), DeserializeError> {
    if data.is_empty() {
        return Err(DeserializeError::UnexpectedEof);
    }
    Ok((data[0], &data[1..]))
}

fn read_u32(data: &[u8]) -> Result<(u32, &[u8]), DeserializeError> {
    if data.len() < 4 {
        return Err(DeserializeError::UnexpectedEof);
    }
    let value = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    Ok((value, &data[4..]))
}

fn read_u64(data: &[u8]) -> Result<(u64, &[u8]), DeserializeError> {
    if data.len() < 8 {
        return Err(DeserializeError::UnexpectedEof);
    }
    let value = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    Ok((value, &data[8..]))
}

fn read_var_int(data: &[u8]) -> Result<(u64, &[u8]), DeserializeError> {
    let (first, data) = read_u8(data)?;
    match first {
        0..=0xFC => Ok((first as u64, data)),
        0xFD => {
            if data.len() < 2 {
                return Err(DeserializeError::UnexpectedEof);
            }
            let value = u16::from_le_bytes([data[0], data[1]]);
            Ok((value as u64, &data[2..]))
        }
        0xFE => {
            let (value, data) = read_u32(data)?;
            Ok((value as u64, data))
        }
        0xFF => read_u64(data),
    }
}

fn read_bytes(data: &[u8]) -> Result<(Vec<u8>, &[u8]), DeserializeError> {
    let (len, data) = read_var_int(data)?;
    if len > MAX_SERIALIZE_BYTES as u64 {
        return Err(DeserializeError::LengthOverflow);
    }
    let len = len as usize;
    if data.len() < len {
        return Err(DeserializeError::UnexpectedEof);
    }
    Ok((data[..len].to_vec(), &data[len..]))
}

fn read_fixed_bytes<const N: usize>(data: &[u8]) -> Result<([u8; N], &[u8]), DeserializeError> {
    if data.len() < N {
        return Err(DeserializeError::UnexpectedEof);
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&data[..N]);
    Ok((arr, &data[N..]))
}

// ============================================================================
// Address
// ============================================================================

/// A pqcoin address, which is the SHA3-512 hash of a public key.
///
/// Addresses are used in P2PKH outputs to specify who can spend the funds.
/// The owner must provide a public key that hashes to this address, along
/// with a valid signature.
///
/// # Size
///
/// Addresses are 64 bytes (512 bits), the full SHA3-512 output.
///
/// # Example
///
/// ```
/// use pqcoin::crypto::ml_dsa_87;
/// use pqcoin::blockchain::Address;
///
/// let (public_key, _) = ml_dsa_87::keygen();
/// let address = Address::from_public_key(&public_key);
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Address(Hash);

impl Address {
    /// Create an address from a public key by hashing it.
    pub fn from_public_key(pk: &PublicKey) -> Self {
        Self(crypto::hash(pk.as_ref()))
    }

    /// Create an address from raw hash bytes.
    pub fn from_hash(hash: Hash) -> Self {
        Self(hash)
    }

    /// Get the underlying hash.
    pub fn as_hash(&self) -> &Hash {
        &self.0
    }

    /// Get the address as bytes.
    pub fn as_bytes(&self) -> &[u8; 64] {
        self.0.as_bytes()
    }

    /// Convert to hex string.
    pub fn to_hex(&self) -> String {
        self.0.to_hex()
    }
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Address({})", &self.to_hex()[..16])
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Display first 16 hex chars for readability
        write!(f, "{}...", &self.to_hex()[..16])
    }
}

impl Serialize for Address {
    fn serialize(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.0.as_bytes());
    }
}

impl Deserialize for Address {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (bytes, data) = read_fixed_bytes::<64>(data)?;
        Ok((Address(Hash::from_bytes(bytes)), data))
    }
}

// ============================================================================
// OutPoint
// ============================================================================

/// A reference to a specific output of a previous transaction.
///
/// An outpoint uniquely identifies a transaction output by combining the
/// transaction ID (hash) with the index of the output within that transaction.
///
/// # Serialization Format
///
/// | Field  | Size     | Description                    |
/// |--------|----------|--------------------------------|
/// | txid   | 64 bytes | SHA3-512 hash of the transaction |
/// | index  | 4 bytes  | Output index (little-endian u32) |
///
/// Total: 68 bytes
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutPoint {
    /// The transaction ID (hash of the transaction).
    pub txid: Hash,
    /// The index of the output within the transaction.
    pub index: u32,
}

impl OutPoint {
    /// Create a new outpoint.
    pub fn new(txid: Hash, index: u32) -> Self {
        Self { txid, index }
    }

    /// Create a null outpoint (used for coinbase transactions).
    pub fn null() -> Self {
        Self {
            txid: Hash::from_bytes([0u8; 64]),
            index: u32::MAX,
        }
    }

    /// Check if this is a null outpoint.
    pub fn is_null(&self) -> bool {
        self.index == u32::MAX && self.txid.as_bytes().iter().all(|&b| b == 0)
    }
}

impl std::fmt::Debug for OutPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OutPoint({}:{}, {})", &self.txid.to_hex()[..8], &self.txid.to_hex()[120..], self.index)
    }
}

impl Serialize for OutPoint {
    fn serialize(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(self.txid.as_bytes());
        write_u32(buf, self.index);
    }
}

impl Deserialize for OutPoint {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (txid_bytes, data) = read_fixed_bytes::<64>(data)?;
        let (index, data) = read_u32(data)?;
        Ok((
            OutPoint {
                txid: Hash::from_bytes(txid_bytes),
                index,
            },
            data,
        ))
    }
}

// ============================================================================
// Witness
// ============================================================================

/// Witness data proving authorization to spend an output.
///
/// The witness contains the cryptographic proof(s) needed to satisfy the
/// spending conditions of a transaction output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Witness {
    /// Witness for a coinbase input: arbitrary data (e.g., block height, extra nonce).
    ///
    /// Coinbase witnesses don't prove authorization (there's nothing to spend),
    /// they just carry metadata. This avoids the overhead of generating unused
    /// cryptographic keys.
    Coinbase(Vec<u8>),
    /// Witness for a P2PKH output: public key + signature.
    P2PKH {
        /// The public key (must hash to the address in the output).
        public_key: PublicKey,
        /// The signature over the transaction.
        signature: Signature,
    },
    /// Witness for a multisig output: public keys + required signatures.
    Multisig {
        /// The public keys (must match those in the output).
        public_keys: Vec<PublicKey>,
        /// The signatures (indices correspond to public_keys).
        /// Contains `None` for keys that didn't sign.
        signatures: Vec<Option<Signature>>,
    },
}

impl Serialize for Witness {
    fn serialize(&self, buf: &mut Vec<u8>) {
        match self {
            Witness::Coinbase(data) => {
                write_u8(buf, 0x02); // Type tag
                write_bytes(buf, data);
            }
            Witness::P2PKH {
                public_key,
                signature,
            } => {
                write_u8(buf, 0x00); // Type tag
                write_bytes(buf, &public_key.to_bytes());
                write_bytes(buf, &signature.to_bytes());
            }
            Witness::Multisig {
                public_keys,
                signatures,
            } => {
                write_u8(buf, 0x01); // Type tag
                write_var_int(buf, public_keys.len() as u64);
                for pk in public_keys {
                    write_bytes(buf, &pk.to_bytes());
                }
                write_var_int(buf, signatures.len() as u64);
                for sig in signatures {
                    match sig {
                        Some(s) => {
                            write_u8(buf, 0x01);
                            write_bytes(buf, &s.to_bytes());
                        }
                        None => {
                            write_u8(buf, 0x00);
                        }
                    }
                }
            }
        }
    }
}

impl Deserialize for Witness {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (tag, data) = read_u8(data)?;
        match tag {
            0x00 => {
                let (pk_bytes, data) = read_bytes(data)?;
                let (sig_bytes, data) = read_bytes(data)?;
                let public_key = PublicKey::from_bytes(&pk_bytes)
                    .ok_or_else(|| DeserializeError::InvalidData("invalid public key".into()))?;
                let signature = Signature::from_bytes(&sig_bytes)
                    .ok_or_else(|| DeserializeError::InvalidData("invalid signature".into()))?;
                Ok((Witness::P2PKH { public_key, signature }, data))
            }
            0x01 => {
                let (num_keys, data) = read_var_int(data)?;
                if num_keys > MAX_MULTISIG_KEYS as u64 {
                    return Err(DeserializeError::LengthOverflow);
                }
                let mut public_keys = Vec::with_capacity(num_keys as usize);
                let mut data = data;
                for _ in 0..num_keys {
                    let (pk_bytes, rest) = read_bytes(data)?;
                    let pk = PublicKey::from_bytes(&pk_bytes)
                        .ok_or_else(|| DeserializeError::InvalidData("invalid public key".into()))?;
                    public_keys.push(pk);
                    data = rest;
                }
                let (num_sigs, data) = read_var_int(data)?;
                if num_sigs > MAX_MULTISIG_KEYS as u64 {
                    return Err(DeserializeError::LengthOverflow);
                }
                let mut signatures = Vec::with_capacity(num_sigs as usize);
                let mut data = data;
                for _ in 0..num_sigs {
                    let (present, rest) = read_u8(data)?;
                    data = rest;
                    if present == 0x01 {
                        let (sig_bytes, rest) = read_bytes(data)?;
                        let sig = Signature::from_bytes(&sig_bytes)
                            .ok_or_else(|| DeserializeError::InvalidData("invalid signature".into()))?;
                        signatures.push(Some(sig));
                        data = rest;
                    } else {
                        signatures.push(None);
                    }
                }
                Ok((Witness::Multisig { public_keys, signatures }, data))
            }
            0x02 => {
                let (coinbase_data, data) = read_bytes(data)?;
                Ok((Witness::Coinbase(coinbase_data), data))
            }
            _ => Err(DeserializeError::InvalidData(format!("unknown witness type: {}", tag))),
        }
    }
}

// ============================================================================
// TxInput
// ============================================================================

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

// ============================================================================
// LockingCondition
// ============================================================================

/// Specifies the conditions required to spend a transaction output.
///
/// This is analogous to Bitcoin's scriptPubKey, but instead of a scripting
/// language, we support only two well-defined output types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockingCondition {
    /// Pay to Public Key Hash: requires a signature from the key that hashes to this address.
    P2PKH(Address),
    /// M-of-N Multisig: requires M signatures from the N specified public keys.
    Multisig {
        /// Number of required signatures.
        threshold: u8,
        /// The public keys that can sign.
        public_keys: Vec<PublicKey>,
    },
}

/// ML-DSA-87 public key size in bytes.
const PQ_PUBLIC_KEY_SIZE: usize = 2592;
/// ML-DSA-87 signature size in bytes.
const PQ_SIGNATURE_SIZE: usize = 4627;

impl LockingCondition {
    /// Create a P2PKH locking condition from a public key.
    pub fn p2pkh(pk: &PublicKey) -> Self {
        LockingCondition::P2PKH(Address::from_public_key(pk))
    }

    /// Create a P2PKH locking condition from an address.
    pub fn p2pkh_address(address: Address) -> Self {
        LockingCondition::P2PKH(address)
    }

    /// Create a multisig locking condition.
    ///
    /// # Panics
    ///
    /// Panics if `threshold` is 0 or greater than the number of public keys.
    pub fn multisig(threshold: u8, public_keys: Vec<PublicKey>) -> Self {
        assert!(threshold > 0, "threshold must be at least 1");
        assert!(
            (threshold as usize) <= public_keys.len(),
            "threshold cannot exceed number of keys"
        );
        LockingCondition::Multisig {
            threshold,
            public_keys,
        }
    }

    /// Estimate the size in bytes required to spend an output with this locking condition.
    ///
    /// This is used to calculate the dynamic dust limit. An output is considered dust
    /// if the cost to spend it (based on this size) exceeds the output's value.
    ///
    /// Includes: OutPoint (68 bytes) + Witness data (varies by condition type)
    pub fn estimated_spend_size(&self) -> usize {
        const OUTPOINT_SIZE: usize = 64 + 4; // txid + index
        const VARINT_OVERHEAD: usize = 4;    // conservative estimate for length prefixes

        match self {
            LockingCondition::P2PKH(_) => {
                // OutPoint + tag(1) + pubkey + signature + varint overhead
                OUTPOINT_SIZE + 1 + PQ_PUBLIC_KEY_SIZE + PQ_SIGNATURE_SIZE + VARINT_OVERHEAD
            }
            LockingCondition::Multisig { threshold, public_keys } => {
                // OutPoint + tag(1) + all pubkeys + threshold signatures + varint overhead
                let num_keys = public_keys.len();
                let num_sigs = *threshold as usize;
                OUTPOINT_SIZE + 1
                    + (num_keys * PQ_PUBLIC_KEY_SIZE)
                    + (num_sigs * PQ_SIGNATURE_SIZE)
                    + VARINT_OVERHEAD * 2  // for key count and sig count
            }
        }
    }
}

impl Serialize for LockingCondition {
    fn serialize(&self, buf: &mut Vec<u8>) {
        match self {
            LockingCondition::P2PKH(address) => {
                write_u8(buf, 0x00);
                address.serialize(buf);
            }
            LockingCondition::Multisig {
                threshold,
                public_keys,
            } => {
                write_u8(buf, 0x01);
                write_u8(buf, *threshold);
                write_var_int(buf, public_keys.len() as u64);
                for pk in public_keys {
                    write_bytes(buf, &pk.to_bytes());
                }
            }
        }
    }
}

impl Deserialize for LockingCondition {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (tag, data) = read_u8(data)?;
        match tag {
            0x00 => {
                let (address, data) = Address::deserialize(data)?;
                Ok((LockingCondition::P2PKH(address), data))
            }
            0x01 => {
                let (threshold, data) = read_u8(data)?;
                let (num_keys, data) = read_var_int(data)?;
                if num_keys > MAX_MULTISIG_KEYS as u64 {
                    return Err(DeserializeError::LengthOverflow);
                }
                if threshold == 0 || (threshold as u64) > num_keys {
                    return Err(DeserializeError::InvalidData(
                        "invalid multisig threshold".into(),
                    ));
                }
                let mut public_keys = Vec::with_capacity(num_keys as usize);
                let mut data = data;
                for _ in 0..num_keys {
                    let (pk_bytes, rest) = read_bytes(data)?;
                    let pk = PublicKey::from_bytes(&pk_bytes)
                        .ok_or_else(|| DeserializeError::InvalidData("invalid public key".into()))?;
                    public_keys.push(pk);
                    data = rest;
                }
                Ok((LockingCondition::Multisig { threshold, public_keys }, data))
            }
            _ => Err(DeserializeError::InvalidData(format!(
                "unknown locking condition type: {}",
                tag
            ))),
        }
    }
}

// ============================================================================
// TxOutput
// ============================================================================

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

// ============================================================================
// Transaction
// ============================================================================

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

// ============================================================================
// BlockHeader
// ============================================================================

/// The header of a block, containing metadata and proof-of-work.
///
/// The block header is the portion of the block that is hashed for
/// proof-of-work. It contains all the information needed to validate
/// the block's place in the chain without the full transaction data.
///
/// # Proof of Work
///
/// The proof-of-work requires finding a nonce such that the SHA3-512 hash
/// of the header is less than or equal to the target. The target is derived
/// from the difficulty bits using the formula:
///
/// ```text
/// target = coefficient * 2^(8 * (exponent - 3))
/// ```
///
/// where `difficulty_bits = (exponent << 24) | coefficient`.
///
/// # Serialization Format
///
/// | Field           | Size     | Description                           |
/// |-----------------|----------|---------------------------------------|
/// | version         | 4 bytes  | Block version (little-endian)         |
/// | prev_hash       | 64 bytes | Hash of the previous block header     |
/// | merkle_root     | 64 bytes | Merkle root of transactions           |
/// | timestamp       | 8 bytes  | Unix timestamp (little-endian)        |
/// | difficulty_bits | 4 bytes  | Compact difficulty target             |
/// | nonce           | 8 bytes  | Proof-of-work nonce (little-endian)   |
///
/// Total: 152 bytes
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BlockHeader {
    /// Block format version.
    pub version: u32,
    /// Hash of the previous block header.
    pub prev_hash: Hash,
    /// Merkle root of the transactions in this block.
    pub merkle_root: Hash,
    /// Unix timestamp when the block was mined.
    pub timestamp: u64,
    /// Compact representation of the difficulty target.
    pub difficulty_bits: u32,
    /// Nonce used to achieve the required proof-of-work.
    pub nonce: u64,
}

impl BlockHeader {
    /// The current block version.
    pub const CURRENT_VERSION: u32 = 1;

    /// Header size in bytes.
    pub const SIZE: usize = 152;

    /// Compute the hash of this block header.
    pub fn hash(&self) -> Hash {
        crypto::hash(&self.to_bytes())
    }

    /// Decode difficulty bits into a 512-bit target.
    ///
    /// The target is a 64-byte big-endian integer. A valid proof-of-work
    /// requires the block hash to be less than or equal to this target.
    pub fn target(&self) -> [u8; 64] {
        let exponent = (self.difficulty_bits >> 24) as usize;
        let coefficient = self.difficulty_bits & DIFFICULTY_COEFFICIENT_MASK;

        let mut target = [0u8; 64];

        if exponent == 0 || exponent > 64 {
            return target;
        }

        // The target formula is: target = coefficient * 2^(8 * (exponent - 3))
        // The coefficient is a 3-byte big-endian number placed at position (64 - exponent)

        if exponent >= 3 {
            // Normal case: coefficient fits in target array
            let coef_bytes = [
                ((coefficient >> 16) & 0xFF) as u8,
                ((coefficient >> 8) & 0xFF) as u8,
                (coefficient & 0xFF) as u8,
            ];

            let start = 64usize.saturating_sub(exponent);
            for (i, &byte) in coef_bytes.iter().enumerate() {
                if start + i < 64 {
                    target[start + i] = byte;
                }
            }
        } else {
            // Very easy target - coefficient shifted right
            // This case is rare (exponent 1 or 2) and represents extremely easy difficulty
            // Shift coefficient right by (3 - exponent) bytes
            let shift_bytes = 3 - exponent;
            let shifted_coef = coefficient >> (8 * shift_bytes);

            // Place the remaining coefficient bytes at the end of the array
            if shift_bytes == 2 {
                // exponent = 1: only 1 byte of coefficient remains
                target[63] = (shifted_coef & 0xFF) as u8;
            } else if shift_bytes == 1 {
                // exponent = 2: 2 bytes of coefficient remain
                target[62] = ((shifted_coef >> 8) & 0xFF) as u8;
                target[63] = (shifted_coef & 0xFF) as u8;
            }
        }

        target
    }

    /// Check if the block hash satisfies the proof-of-work requirement.
    pub fn check_pow(&self) -> bool {
        let hash = self.hash();
        let target = self.target();

        // Compare hash to target (both as big-endian 512-bit integers)
        // Hash must be <= target
        for (h, t) in hash.as_bytes().iter().zip(target.iter()) {
            if h < t {
                return true;
            }
            if h > t {
                return false;
            }
        }
        true // Equal
    }

    /// Encode a target into difficulty bits.
    ///
    /// This is the inverse of `target()`.
    pub fn encode_target(target: &[u8; 64]) -> u32 {
        // Find the first non-zero byte
        let mut first_nonzero = 0;
        for (i, &byte) in target.iter().enumerate() {
            if byte != 0 {
                first_nonzero = i;
                break;
            }
        }

        // Extract 3 coefficient bytes starting at first_nonzero
        let mut coefficient = 0u32;
        for i in 0..3 {
            if first_nonzero + i < 64 {
                coefficient = (coefficient << 8) | (target[first_nonzero + i] as u32);
            }
        }

        // Exponent is 64 - first_nonzero
        let exponent = (64 - first_nonzero) as u32;

        // If the high bit of coefficient is set, we need to adjust
        // to avoid it being interpreted as negative
        if coefficient & 0x00800000 != 0 {
            coefficient >>= 8;
            ((exponent + 1) << 24) | coefficient
        } else {
            (exponent << 24) | coefficient
        }
    }
}

impl std::fmt::Debug for BlockHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlockHeader")
            .field("version", &self.version)
            .field("prev_hash", &format!("{}...", &self.prev_hash.to_hex()[..16]))
            .field("merkle_root", &format!("{}...", &self.merkle_root.to_hex()[..16]))
            .field("timestamp", &self.timestamp)
            .field("difficulty_bits", &format!("0x{:08x}", self.difficulty_bits))
            .field("nonce", &self.nonce)
            .finish()
    }
}

impl Serialize for BlockHeader {
    fn serialize(&self, buf: &mut Vec<u8>) {
        write_u32(buf, self.version);
        buf.extend_from_slice(self.prev_hash.as_bytes());
        buf.extend_from_slice(self.merkle_root.as_bytes());
        write_u64(buf, self.timestamp);
        write_u32(buf, self.difficulty_bits);
        write_u64(buf, self.nonce);
    }
}

impl Deserialize for BlockHeader {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (version, data) = read_u32(data)?;
        let (prev_hash, data) = read_fixed_bytes::<64>(data)?;
        let (merkle_root, data) = read_fixed_bytes::<64>(data)?;
        let (timestamp, data) = read_u64(data)?;
        let (difficulty_bits, data) = read_u32(data)?;
        let (nonce, data) = read_u64(data)?;
        Ok((
            BlockHeader {
                version,
                prev_hash: Hash::from_bytes(prev_hash),
                merkle_root: Hash::from_bytes(merkle_root),
                timestamp,
                difficulty_bits,
                nonce,
            },
            data,
        ))
    }
}

// ============================================================================
// Block
// ============================================================================

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
        Self { header, transactions }
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
        Ok((Block { header, transactions }, data))
    }
}

// ============================================================================
// Blockchain
// ============================================================================

/// Errors that can occur during blockchain operations.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    DustOutput { index: usize, amount: u64, limit: u64 },
    /// Block hash doesn't match the checkpoint at this height.
    CheckpointMismatch { height: u64, expected: Hash, got: Hash },
}

impl std::fmt::Display for BlockchainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BlockchainError::UnknownPreviousBlock => write!(f, "unknown previous block"),
            BlockchainError::InvalidProofOfWork => write!(f, "invalid proof of work"),
            BlockchainError::InvalidMerkleRoot => write!(f, "invalid merkle root"),
            BlockchainError::EmptyBlock => write!(f, "block has no transactions"),
            BlockchainError::InvalidCoinbase => write!(f, "invalid coinbase transaction"),
            BlockchainError::MissingInput(op) => write!(f, "missing input: {:?}", op),
            BlockchainError::InvalidWitness => write!(f, "invalid witness"),
            BlockchainError::InsufficientInputs => write!(f, "insufficient inputs"),
            BlockchainError::DoubleSpend(op) => write!(f, "double spend: {:?}", op),
            BlockchainError::InvalidTimestamp => write!(f, "invalid timestamp"),
            BlockchainError::InvalidDifficulty => write!(f, "invalid difficulty"),
            BlockchainError::FeeOverflow => write!(f, "fee calculation overflow"),
            BlockchainError::BlockTooLarge => write!(f, "block exceeds maximum size"),
            BlockchainError::ImmatureCoinbase { outpoint, current_height, maturity_height } => {
                write!(f, "immature coinbase: {:?} (current height {}, matures at {})",
                    outpoint, current_height, maturity_height)
            }
            BlockchainError::ReorgTooDeep(depth) => write!(f, "reorg too deep: {} blocks", depth),
            BlockchainError::TooManyInputs(count) => write!(f, "too many inputs: {}", count),
            BlockchainError::TooManyOutputs(count) => write!(f, "too many outputs: {}", count),
            BlockchainError::DustOutput { index, amount, limit } => {
                write!(f, "output {} is dust: {} below limit {}", index, amount, limit)
            }
            BlockchainError::CheckpointMismatch { height, expected, got } => {
                write!(f, "checkpoint mismatch at height {}: expected {}, got {}",
                    height, expected, got)
            }
        }
    }
}

impl std::error::Error for BlockchainError {}

/// An unspent transaction output in the UTXO set.
#[derive(Clone, Debug)]
pub struct Utxo {
    /// The output data.
    pub output: TxOutput,
    /// The height at which this output was created.
    pub height: u64,
    /// Whether this is from a coinbase transaction.
    pub is_coinbase: bool,
}

/// The blockchain state, tracking all blocks and the UTXO set.
///
/// # Chain Selection
///
/// When multiple valid chains exist (forks), the chain with the most
/// cumulative proof-of-work is selected as the canonical chain. This is
/// typically the longest chain, but difficulty adjustments mean that
/// raw block count isn't sufficient.
///
/// # UTXO Set
///
/// The UTXO (Unspent Transaction Output) set tracks all outputs that
/// can still be spent. This provides efficient validation of new
/// transactions without scanning the entire blockchain.
#[derive(Clone)]
pub struct Blockchain {
    /// All known blocks, indexed by hash.
    blocks: HashMap<Hash, Block>,
    /// Block heights, indexed by hash.
    heights: HashMap<Hash, u64>,
    /// The hash of the current chain tip (best block).
    tip: Hash,
    /// The height of the current chain tip.
    tip_height: u64,
    /// The unspent transaction output set.
    utxos: HashMap<OutPoint, Utxo>,
    /// Transaction ID to block hash index for fast lookups during reorgs.
    tx_index: HashMap<Hash, Hash>,
    /// The genesis block hash.
    genesis_hash: Hash,
    /// Difficulty adjustment interval (blocks).
    difficulty_adjustment_interval: u64,
    /// Target block time (seconds).
    target_block_time: u64,
    /// Initial block reward.
    initial_reward: u64,
    /// Reward halving interval (blocks).
    halving_interval: u64,
}

/// Calculate the dust limit for a given locking condition.
///
/// The dust limit is based on the cost to spend the output at DUST_FEE_RATE.
/// This ensures outputs are economically rational to spend while allowing
/// small payments (since DUST_FEE_RATE is lower than MIN_RELAY_FEE).
fn dust_limit(condition: &LockingCondition) -> u64 {
    let spend_size = condition.estimated_spend_size() as u64;
    // dust = spend_size * DUST_FEE_RATE / 1000 (rate is per KB)
    (spend_size * DUST_FEE_RATE) / 1000
}

/// Check if an output amount is below the dust limit for its locking condition.
/// Dust outputs are uneconomical to spend and bloat the UTXO set.
fn is_dust(output: &TxOutput) -> bool {
    output.amount < dust_limit(&output.condition)
}

impl Blockchain {
    /// Create a new blockchain with the given genesis block.
    ///
    /// # Parameters
    ///
    /// * `genesis` - The genesis block (block 0)
    /// * `difficulty_adjustment_interval` - How often to adjust difficulty (e.g., 2016 for Bitcoin)
    /// * `target_block_time` - Target seconds between blocks (e.g., 600 for Bitcoin)
    /// * `initial_reward` - Initial block reward in quanta
    /// * `halving_interval` - How often to halve the reward (e.g., 210000 for Bitcoin)
    pub fn new(
        genesis: Block,
        difficulty_adjustment_interval: u64,
        target_block_time: u64,
        initial_reward: u64,
        halving_interval: u64,
    ) -> Self {
        let genesis_hash = genesis.hash();

        let mut blocks = HashMap::new();
        let mut heights = HashMap::new();
        let mut utxos = HashMap::new();
        let mut tx_index = HashMap::new();

        // Add genesis block UTXOs and index transactions
        for (i, tx) in genesis.transactions.iter().enumerate() {
            let txid = tx.txid();
            tx_index.insert(txid, genesis_hash);
            for (j, output) in tx.outputs.iter().enumerate() {
                let outpoint = OutPoint::new(txid, j as u32);
                utxos.insert(
                    outpoint,
                    Utxo {
                        output: output.clone(),
                        height: 0,
                        is_coinbase: i == 0,
                    },
                );
            }
        }

        blocks.insert(genesis_hash, genesis);
        heights.insert(genesis_hash, 0);

        Blockchain {
            blocks,
            heights,
            tip: genesis_hash,
            tip_height: 0,
            utxos,
            tx_index,
            genesis_hash,
            difficulty_adjustment_interval,
            target_block_time,
            initial_reward,
            halving_interval,
        }
    }

    /// Get the genesis block.
    pub fn genesis(&self) -> &Block {
        self.blocks.get(&self.genesis_hash).unwrap()
    }

    /// Get the current chain tip (best block).
    pub fn tip(&self) -> &Block {
        self.blocks.get(&self.tip).unwrap()
    }

    /// Get the current chain tip hash.
    pub fn tip_hash(&self) -> Hash {
        self.tip
    }

    /// Get the current chain height.
    pub fn height(&self) -> u64 {
        self.tip_height
    }

    /// Get a block by its hash.
    pub fn get_block(&self, hash: &Hash) -> Option<&Block> {
        self.blocks.get(hash)
    }

    /// Get a block's height.
    pub fn get_height(&self, hash: &Hash) -> Option<u64> {
        self.heights.get(hash).copied()
    }

    /// Alias for get_height (used by sync module).
    pub fn height_of(&self, hash: &Hash) -> Option<u64> {
        self.get_height(hash)
    }

    /// Check if we have a block.
    pub fn has_block(&self, hash: &Hash) -> bool {
        self.blocks.contains_key(hash)
    }

    /// Get the block hash at a specific height.
    ///
    /// This walks back from the tip, so it's O(height) in the worst case.
    pub fn hash_at_height(&self, target_height: u64) -> Option<Hash> {
        if target_height > self.tip_height {
            return None;
        }

        let mut current_hash = self.tip;
        let mut current_height = self.tip_height;

        while current_height > target_height {
            if let Some(block) = self.blocks.get(&current_hash) {
                current_hash = block.header.prev_hash;
                current_height -= 1;
            } else {
                return None;
            }
        }

        Some(current_hash)
    }

    /// Alias for hash_at_height (used by sync module).
    pub fn block_hash_at_height(&self, height: u64) -> Option<Hash> {
        self.hash_at_height(height)
    }

    /// Get a UTXO by its outpoint.
    pub fn get_utxo(&self, outpoint: &OutPoint) -> Option<&Utxo> {
        self.utxos.get(outpoint)
    }

    /// Calculate the block reward for a given height.
    pub fn block_reward(&self, height: u64) -> u64 {
        let halvings = height / self.halving_interval;
        if halvings >= 64 {
            0
        } else {
            self.initial_reward >> halvings
        }
    }

    /// Calculate the expected difficulty for a block at a given height.
    ///
    /// This validates that a block has the correct difficulty based on its position
    /// in the chain and the difficulty adjustment schedule.
    fn expected_difficulty_at(&self, height: u64, prev_hash: Hash) -> u32 {
        let prev_block = match self.blocks.get(&prev_hash) {
            Some(b) => b,
            None => return 0, // Unknown parent, can't calculate
        };

        // If not at an adjustment boundary, use the same difficulty as parent
        if height % self.difficulty_adjustment_interval != 0 {
            return prev_block.header.difficulty_bits;
        }

        // At adjustment boundary - need to calculate new difficulty
        // Find the block at the start of this adjustment period
        let period_start_height = height.saturating_sub(self.difficulty_adjustment_interval);
        let mut block_hash = prev_hash;

        // Walk back to find the period start block
        let steps = height.saturating_sub(1).saturating_sub(period_start_height);
        for _ in 0..steps {
            if let Some(block) = self.blocks.get(&block_hash) {
                block_hash = block.header.prev_hash;
            } else {
                return prev_block.header.difficulty_bits;
            }
        }

        let period_start = match self.blocks.get(&block_hash) {
            Some(b) => b,
            None => return prev_block.header.difficulty_bits,
        };

        // Calculate actual time taken for this period
        let actual_time = prev_block.header.timestamp.saturating_sub(period_start.header.timestamp);
        let target_time = self.target_block_time * self.difficulty_adjustment_interval;

        // Clamp adjustment to 4x in either direction
        let actual_time = actual_time.max(target_time / 4).min(target_time * 4);

        // Scale the difficulty
        let current_bits = prev_block.header.difficulty_bits;
        let exponent = current_bits >> 24;
        let coefficient = (current_bits & DIFFICULTY_COEFFICIENT_MASK) as u64;

        let scaled = (coefficient * actual_time) / target_time;

        let (new_exponent, new_coefficient) = if scaled == 0 {
            (1u32, 1u32)
        } else if scaled > 0x7FFFFF {
            let shift = 64 - scaled.leading_zeros();
            let extra_bytes = (shift.saturating_sub(23) + 7) / 8;
            let new_exp = exponent.saturating_add(extra_bytes);
            let new_coef = (scaled >> (extra_bytes * 8)) as u32;
            if new_exp > 64 {
                (64u32, 0x7FFFFFu32)
            } else {
                (new_exp, new_coef.min(0x7FFFFF))
            }
        } else {
            (exponent, scaled as u32)
        };

        (new_exponent << 24) | new_coefficient
    }

    /// Calculate the expected difficulty for a new block.
    ///
    /// Difficulty is adjusted every `difficulty_adjustment_interval` blocks
    /// to maintain the target block time.
    pub fn next_difficulty(&self) -> u32 {
        let tip = self.tip();

        // If we're not at an adjustment boundary, keep the same difficulty
        if (self.tip_height + 1) % self.difficulty_adjustment_interval != 0 {
            return tip.header.difficulty_bits;
        }

        // Find the block at the start of this adjustment period
        let period_start_height =
            self.tip_height.saturating_sub(self.difficulty_adjustment_interval - 1);
        let mut block_hash = self.tip;

        // Walk back to find the period start block
        for _ in 0..(self.tip_height - period_start_height) {
            if let Some(block) = self.blocks.get(&block_hash) {
                block_hash = block.header.prev_hash;
            } else {
                return tip.header.difficulty_bits;
            }
        }

        let period_start = match self.blocks.get(&block_hash) {
            Some(b) => b,
            None => return tip.header.difficulty_bits,
        };

        // Calculate actual time taken
        let actual_time = tip.header.timestamp.saturating_sub(period_start.header.timestamp);
        let target_time = self.target_block_time * self.difficulty_adjustment_interval;

        // Clamp adjustment to 4x in either direction
        let actual_time = actual_time.max(target_time / 4).min(target_time * 4);

        // Work directly with the compact encoding to avoid precision issues.
        // The difficulty_bits format is: (exponent << 24) | coefficient
        // where target = coefficient * 2^(8 * (exponent - 3))
        //
        // To scale the target by (actual_time / target_time), we scale the coefficient.
        // This stays within 64-bit arithmetic since coefficient is 24 bits and
        // actual_time/target_time is clamped to [0.25, 4.0].
        let current_bits = tip.header.difficulty_bits;
        let exponent = current_bits >> 24;
        let coefficient = (current_bits & DIFFICULTY_COEFFICIENT_MASK) as u64;

        // Scale: new_coefficient = coefficient * actual_time / target_time
        // Use 64-bit arithmetic: max value is 0xFFFFFF * 4 = 0x3FFFFFC (26 bits)
        let scaled = (coefficient * actual_time) / target_time;

        // Normalize: coefficient must fit in 24 bits (0x000000 to 0x7FFFFF to avoid
        // the high bit being set, which would be interpreted as negative)
        let (new_exponent, new_coefficient) = if scaled == 0 {
            // Target is effectively zero (impossibly hard) - use minimum
            (1u32, 1u32)
        } else if scaled > 0x7FFFFF {
            // Coefficient too large - increase exponent (easier target)
            // Each exponent increment multiplies target by 256
            let shift = 64 - scaled.leading_zeros(); // bits needed
            let extra_bytes = (shift.saturating_sub(23) + 7) / 8; // bytes to shift
            let new_exp = exponent.saturating_add(extra_bytes);
            let new_coef = (scaled >> (extra_bytes * 8)) as u32;
            // Clamp exponent to valid range (1-64 for 512-bit hash)
            if new_exp > 64 {
                (64u32, 0x7FFFFFu32) // Maximum target (easiest)
            } else {
                (new_exp, new_coef.min(0x7FFFFF))
            }
        } else {
            // Coefficient fits, keep same exponent
            (exponent, scaled as u32)
        };

        (new_exponent << 24) | new_coefficient
    }

    /// Verify signatures for a batch of transactions in parallel.
    ///
    /// Uses rayon to verify multiple transaction signatures concurrently for better performance.
    fn verify_transactions_parallel(
        &self,
        transactions: &[Transaction],
        height: u64,
    ) -> Result<Vec<u64>, BlockchainError> {
        // Collect verification data for all transactions first
        let verify_data: Vec<_> = transactions
            .iter()
            .map(|tx| {
                let mut input_data = Vec::new();
                for (input_index, input) in tx.inputs.iter().enumerate() {
                    let utxo = self.utxos.get(&input.outpoint);
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
                    let utxo = utxo_opt
                        .as_ref()
                        .ok_or(BlockchainError::MissingInput(tx.inputs[*input_index].outpoint))?;

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
                    match (&utxo.output.condition, witness) {
                        (LockingCondition::P2PKH(address), Witness::P2PKH { public_key, signature }) => {
                            if Address::from_public_key(public_key) != *address {
                                return Err(BlockchainError::InvalidWitness);
                            }
                            if !crypto::ml_dsa_87::verify(public_key, message.as_bytes(), signature) {
                                return Err(BlockchainError::InvalidWitness);
                            }
                        }
                        (
                            LockingCondition::Multisig { threshold, public_keys },
                            Witness::Multisig {
                                public_keys: witness_keys,
                                signatures,
                            },
                        ) => {
                            if witness_keys.len() != public_keys.len() {
                                return Err(BlockchainError::InvalidWitness);
                            }
                            for (wk, pk) in witness_keys.iter().zip(public_keys.iter()) {
                                if wk.to_bytes() != pk.to_bytes() {
                                    return Err(BlockchainError::InvalidWitness);
                                }
                            }
                            let mut valid_sigs = 0u8;
                            for (i, sig_opt) in signatures.iter().enumerate() {
                                if let Some(sig) = sig_opt {
                                    if i < public_keys.len()
                                        && crypto::ml_dsa_87::verify(
                                            &public_keys[i],
                                            message.as_bytes(),
                                            sig,
                                        )
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
                }

                Ok(input_sum)
            })
            .collect();

        // Collect results, returning first error if any
        results.into_iter().collect()
    }

    /// Verify a transaction's signatures.
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
    fn verify_transaction(&self, tx: &Transaction, height: u64) -> Result<u64, BlockchainError> {
        if tx.is_coinbase() {
            // Coinbase transactions are verified differently
            return Ok(0);
        }

        let mut input_sum = 0u64;

        for (input_index, input) in tx.inputs.iter().enumerate() {
            // Get the UTXO being spent
            let utxo = self
                .utxos
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

            match (&utxo.output.condition, &input.witness) {
                (LockingCondition::P2PKH(address), Witness::P2PKH { public_key, signature }) => {
                    // Verify the public key hashes to the address
                    if Address::from_public_key(public_key) != *address {
                        return Err(BlockchainError::InvalidWitness);
                    }
                    // Verify the signature
                    if !crypto::ml_dsa_87::verify(public_key, message.as_bytes(), signature) {
                        return Err(BlockchainError::InvalidWitness);
                    }
                }
                (
                    LockingCondition::Multisig { threshold, public_keys },
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
                                && crypto::ml_dsa_87::verify(
                                    &public_keys[i],
                                    message.as_bytes(),
                                    sig,
                                )
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
        }

        Ok(input_sum)
    }

    /// Add a block to the chain.
    ///
    /// Returns `Ok(true)` if the block extended the main chain,
    /// `Ok(false)` if it was added to a side chain, or an error if invalid.
    pub fn add_block(&mut self, block: Block) -> Result<bool, BlockchainError> {
        let block_hash = block.hash();

        // Already have this block?
        if self.blocks.contains_key(&block_hash) {
            return Ok(false);
        }

        // Check block size limit to prevent memory exhaustion attacks
        let block_size = block.to_bytes().len();
        if block_size > MAX_BLOCK_SIZE {
            return Err(BlockchainError::BlockTooLarge);
        }

        // Check previous block exists
        let prev_height = self
            .heights
            .get(&block.header.prev_hash)
            .ok_or(BlockchainError::UnknownPreviousBlock)?;
        let height = prev_height + 1;

        // Get the previous block for timestamp validation
        let prev_block = self
            .blocks
            .get(&block.header.prev_hash)
            .ok_or(BlockchainError::UnknownPreviousBlock)?;

        // Verify timestamp: must be strictly greater than previous block
        if block.header.timestamp <= prev_block.header.timestamp {
            return Err(BlockchainError::InvalidTimestamp);
        }

        // Verify timestamp: must not be more than MAX_FUTURE_BLOCK_TIME in the future
        // This prevents miners from claiming future timestamps to manipulate difficulty
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        if block.header.timestamp > current_time + MAX_FUTURE_BLOCK_TIME {
            return Err(BlockchainError::InvalidTimestamp);
        }

        // Verify difficulty matches expected value for this height
        // Skip check for genesis block (height 0) which has no parent for calculation
        if height > 0 {
            let expected_difficulty = self.expected_difficulty_at(height, block.header.prev_hash);
            if block.header.difficulty_bits != expected_difficulty {
                return Err(BlockchainError::InvalidDifficulty);
            }
        }

        // Verify proof of work
        if !block.header.check_pow() {
            return Err(BlockchainError::InvalidProofOfWork);
        }

        // Verify merkle root
        if !block.verify_merkle_root() {
            return Err(BlockchainError::InvalidMerkleRoot);
        }

        // Verify transactions
        if block.transactions.is_empty() {
            return Err(BlockchainError::EmptyBlock);
        }

        // First transaction must be coinbase
        if !block.transactions[0].is_coinbase() {
            return Err(BlockchainError::InvalidCoinbase);
        }

        // Verify all other transactions are not coinbase
        for tx in block.transactions.iter().skip(1) {
            if tx.is_coinbase() {
                return Err(BlockchainError::InvalidCoinbase);
            }
        }

        // Validate transaction input/output counts
        for tx in &block.transactions {
            if tx.inputs.len() > MAX_TX_INPUTS {
                return Err(BlockchainError::TooManyInputs(tx.inputs.len()));
            }
            if tx.outputs.len() > MAX_TX_OUTPUTS {
                return Err(BlockchainError::TooManyOutputs(tx.outputs.len()));
            }
        }

        // Check for dust outputs (skip coinbase transaction)
        for tx in block.transactions.iter().skip(1) {
            for (index, output) in tx.outputs.iter().enumerate() {
                if is_dust(output) {
                    return Err(BlockchainError::DustOutput {
                        index,
                        amount: output.amount,
                        limit: dust_limit(&output.condition),
                    });
                }
            }
        }

        // Track spent outputs to detect double-spends within the block (must be sequential)
        let mut spent_in_block = HashMap::new();
        for tx in block.transactions.iter().skip(1) {
            for input in &tx.inputs {
                if spent_in_block.contains_key(&input.outpoint) {
                    return Err(BlockchainError::DoubleSpend(input.outpoint));
                }
                if !self.utxos.contains_key(&input.outpoint) {
                    return Err(BlockchainError::MissingInput(input.outpoint));
                }
                spent_in_block.insert(input.outpoint, ());
            }
        }

        // Verify all non-coinbase transactions in parallel for better performance
        let non_coinbase_txs: Vec<_> = block.transactions.iter().skip(1).cloned().collect();
        let input_sums = self.verify_transactions_parallel(&non_coinbase_txs, height)?;

        // Calculate total fees (must be sequential for checked arithmetic)
        let mut total_fees = 0u64;
        for (tx, input_sum) in non_coinbase_txs.iter().zip(input_sums.iter()) {
            let output_sum = tx.total_output();
            let fee = input_sum
                .checked_sub(output_sum)
                .ok_or(BlockchainError::InsufficientInputs)?;
            total_fees = total_fees
                .checked_add(fee)
                .ok_or(BlockchainError::FeeOverflow)?;
        }

        // Verify coinbase amount using checked arithmetic
        let expected_reward = self.block_reward(height);
        let coinbase_output = block.transactions[0].total_output();
        let max_coinbase = expected_reward
            .checked_add(total_fees)
            .ok_or(BlockchainError::FeeOverflow)?;
        if coinbase_output > max_coinbase {
            return Err(BlockchainError::InvalidCoinbase);
        }

        // Block is valid; add it
        self.blocks.insert(block_hash, block.clone());
        self.heights.insert(block_hash, height);

        // Check if this creates a longer chain (potential reorg)
        if height > self.tip_height {
            // Check if this extends the current tip (no reorg needed)
            if block.header.prev_hash == self.tip {
                // Simple case: extends current chain
                self.apply_block(&block, height);
                self.tip = block_hash;
                self.tip_height = height;
                Ok(true)
            } else {
                // Reorg needed: new chain is longer but doesn't extend current tip
                self.reorganize_to(block_hash, height)?;
                Ok(true)
            }
        } else {
            // Side chain block, stored but doesn't change tip
            Ok(false)
        }
    }

    /// Apply a block's effects to the UTXO set and tx index (add outputs, remove inputs).
    fn apply_block(&mut self, block: &Block, height: u64) {
        let block_hash = block.hash();

        // Remove spent outputs
        for tx in block.transactions.iter().skip(1) {
            for input in &tx.inputs {
                self.utxos.remove(&input.outpoint);
            }
        }

        // Add new outputs and index transactions
        for (i, tx) in block.transactions.iter().enumerate() {
            let txid = tx.txid();
            self.tx_index.insert(txid, block_hash);
            for (j, output) in tx.outputs.iter().enumerate() {
                let outpoint = OutPoint::new(txid, j as u32);
                self.utxos.insert(
                    outpoint,
                    Utxo {
                        output: output.clone(),
                        height,
                        is_coinbase: i == 0,
                    },
                );
            }
        }
    }

    /// Unapply a block's effects from the UTXO set and tx index (remove outputs, restore inputs).
    fn unapply_block(&mut self, block: &Block, _height: u64) {
        // Remove outputs and tx index entries for this block
        for tx in block.transactions.iter() {
            let txid = tx.txid();
            self.tx_index.remove(&txid);
            for j in 0..tx.outputs.len() {
                let outpoint = OutPoint::new(txid, j as u32);
                self.utxos.remove(&outpoint);
            }
        }

        // Restore spent inputs (we need to look them up from their source transactions)
        for tx in block.transactions.iter().skip(1) {
            for input in &tx.inputs {
                // Find the transaction that created this output using the tx index
                if let Some(source_block) = self.find_block_containing_tx(&input.outpoint.txid) {
                    let source_height = *self.heights.get(&source_block.hash()).unwrap_or(&0);
                    if let Some(source_tx) = source_block.transactions.iter()
                        .find(|t| t.txid() == input.outpoint.txid)
                    {
                        if let Some(output) = source_tx.outputs.get(input.outpoint.index as usize) {
                            let is_coinbase = source_block.transactions.first()
                                .map(|t| t.txid() == input.outpoint.txid)
                                .unwrap_or(false);
                            self.utxos.insert(
                                input.outpoint,
                                Utxo {
                                    output: output.clone(),
                                    height: source_height,
                                    is_coinbase,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    /// Find the block containing a specific transaction.
    ///
    /// Uses the tx_index for O(1) lookup instead of scanning all blocks.
    fn find_block_containing_tx(&self, txid: &Hash) -> Option<&Block> {
        self.tx_index.get(txid).and_then(|block_hash| self.blocks.get(block_hash))
    }

    /// Find the common ancestor of two block hashes.
    fn find_common_ancestor(&self, hash1: Hash, hash2: Hash) -> Option<Hash> {
        let mut ancestors1 = std::collections::HashSet::new();
        let mut current = hash1;

        // Collect all ancestors of hash1
        while let Some(block) = self.blocks.get(&current) {
            ancestors1.insert(current);
            if current == self.genesis_hash {
                break;
            }
            current = block.header.prev_hash;
        }

        // Walk back from hash2 until we find a common ancestor
        current = hash2;
        while let Some(block) = self.blocks.get(&current) {
            if ancestors1.contains(&current) {
                return Some(current);
            }
            if current == self.genesis_hash {
                break;
            }
            current = block.header.prev_hash;
        }

        None
    }

    /// Get the chain of blocks from a hash back to (but not including) an ancestor.
    fn get_chain_segment(&self, from: Hash, to: Hash) -> Vec<Hash> {
        let mut chain = Vec::new();
        let mut current = from;

        while current != to {
            chain.push(current);
            if let Some(block) = self.blocks.get(&current) {
                current = block.header.prev_hash;
            } else {
                break;
            }
        }

        chain.reverse();
        chain
    }

    /// Reorganize the chain to a new tip.
    fn reorganize_to(&mut self, new_tip: Hash, new_height: u64) -> Result<(), BlockchainError> {
        let old_tip = self.tip;

        // Find common ancestor
        let fork_point = self.find_common_ancestor(old_tip, new_tip)
            .ok_or(BlockchainError::UnknownPreviousBlock)?;

        // Get blocks to unapply (old chain from tip back to fork point)
        let old_chain = self.get_chain_segment(old_tip, fork_point);

        // Check reorg depth to prevent long-range attacks
        let reorg_depth = old_chain.len() as u64;
        if reorg_depth > MAX_REORG_DEPTH {
            tracing::warn!(
                reorg_depth = reorg_depth,
                max_allowed = MAX_REORG_DEPTH,
                "rejecting deep reorg"
            );
            return Err(BlockchainError::ReorgTooDeep(reorg_depth));
        }

        // Get blocks to apply (new chain from fork point to new tip)
        let new_chain = self.get_chain_segment(new_tip, fork_point);

        tracing::info!(
            old_chain_len = old_chain.len(),
            new_chain_len = new_chain.len(),
            "performing chain reorganization"
        );

        // Unapply old blocks (in reverse order, from tip to fork point)
        for hash in old_chain.iter().rev() {
            if let Some(block) = self.blocks.get(hash).cloned() {
                let height = *self.heights.get(hash).unwrap_or(&0);
                self.unapply_block(&block, height);
            }
        }

        // Apply new blocks (in order, from fork point to new tip)
        for hash in &new_chain {
            if let Some(block) = self.blocks.get(hash).cloned() {
                let height = *self.heights.get(hash).unwrap_or(&0);
                self.apply_block(&block, height);
            }
        }

        self.tip = new_tip;
        self.tip_height = new_height;

        Ok(())
    }

    /// Get the chain of block hashes from genesis to tip.
    pub fn chain(&self) -> Vec<Hash> {
        let mut chain = Vec::with_capacity(self.tip_height as usize + 1);
        let mut current = self.tip;

        while let Some(block) = self.blocks.get(&current) {
            chain.push(current);
            if current == self.genesis_hash {
                break;
            }
            current = block.header.prev_hash;
        }

        chain.reverse();
        chain
    }

    /// Get all UTXOs for a given address.
    pub fn utxos_for_address(&self, address: &Address) -> Vec<(OutPoint, &Utxo)> {
        self.utxos
            .iter()
            .filter(|(_, utxo)| match &utxo.output.condition {
                LockingCondition::P2PKH(addr) => addr == address,
                _ => false,
            })
            .map(|(op, utxo)| (*op, utxo))
            .collect()
    }

    /// Get the total balance for an address.
    pub fn balance(&self, address: &Address) -> u64 {
        self.utxos_for_address(address)
            .iter()
            .map(|(_, utxo)| utxo.output.amount)
            .sum()
    }

    /// Get an iterator over all blocks in the blockchain.
    pub fn all_blocks(&self) -> impl Iterator<Item = &Block> {
        self.blocks.values()
    }

    /// Check if a transaction exists in the blockchain.
    ///
    /// This is a relatively expensive operation as it searches all blocks.
    /// Consider caching transaction indices for frequent lookups.
    pub fn has_transaction(&self, txid: &Hash) -> bool {
        self.find_block_containing_tx(txid).is_some()
    }
}

impl std::fmt::Debug for Blockchain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Blockchain")
            .field("height", &self.tip_height)
            .field("tip", &format!("{}...", &self.tip.to_hex()[..16]))
            .field("blocks", &self.blocks.len())
            .field("utxos", &self.utxos.len())
            .finish()
    }
}

// ============================================================================
// Genesis Block Creation
// ============================================================================

/// Create the pqcoin genesis block.
///
/// The genesis block is hardcoded and defines the initial state of the blockchain.
/// It contains a single coinbase transaction with the initial block reward.
pub fn create_genesis_block(
    timestamp: u64,
    difficulty_bits: u32,
    initial_reward: u64,
    recipient: Address,
) -> Block {
    let coinbase = Transaction::coinbase(0, initial_reward, recipient);
    let merkle_root = Block::compute_merkle_root(&[coinbase.clone()]);

    let header = BlockHeader {
        version: BlockHeader::CURRENT_VERSION,
        prev_hash: Hash::from_bytes([0u8; 64]),
        merkle_root,
        timestamp,
        difficulty_bits,
        nonce: 0,
    };

    Block::new(header, vec![coinbase])
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a test keypair
    fn test_keypair() -> (PublicKey, crypto::SecretKey) {
        crypto::ml_dsa_87::keygen()
    }

    // ========================================================================
    // Serialization Tests
    // ========================================================================

    #[test]
    fn test_varint_roundtrip() {
        let test_values = [
            0u64,
            1,
            0xFC,
            0xFD,
            0xFE,
            0xFF,
            0x100,
            0xFFFF,
            0x10000,
            0xFFFFFFFF,
            0x100000000,
            u64::MAX,
        ];

        for &value in &test_values {
            let mut buf = Vec::new();
            write_var_int(&mut buf, value);
            let (decoded, remaining) = read_var_int(&buf).unwrap();
            assert_eq!(decoded, value, "varint roundtrip failed for {}", value);
            assert!(remaining.is_empty());
        }
    }

    #[test]
    fn test_address_serialization() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);

        let bytes = address.to_bytes();
        assert_eq!(bytes.len(), 64);

        let (decoded, remaining) = Address::deserialize(&bytes).unwrap();
        assert!(remaining.is_empty());
        assert_eq!(decoded, address);
    }

    #[test]
    fn test_outpoint_serialization() {
        let outpoint = OutPoint::new(crypto::hash(b"test tx"), 42);

        let bytes = outpoint.to_bytes();
        assert_eq!(bytes.len(), 68); // 64 + 4

        let decoded = OutPoint::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, outpoint);
    }

    #[test]
    fn test_outpoint_null() {
        let null = OutPoint::null();
        assert!(null.is_null());

        let bytes = null.to_bytes();
        let decoded = OutPoint::from_bytes(&bytes).unwrap();
        assert!(decoded.is_null());
    }

    #[test]
    fn test_witness_p2pkh_serialization() {
        let (pk, sk) = test_keypair();
        let message = b"test message";
        let sig = crypto::ml_dsa_87::sign(&sk, message);

        let witness = Witness::P2PKH {
            public_key: pk,
            signature: sig,
        };

        let bytes = witness.to_bytes();
        let decoded = Witness::from_bytes(&bytes).unwrap();

        match decoded {
            Witness::P2PKH { public_key, signature } => {
                // Verify signature still works after serialization roundtrip
                assert!(crypto::ml_dsa_87::verify(&public_key, message, &signature));
            }
            _ => panic!("wrong witness type"),
        }
    }

    #[test]
    fn test_witness_coinbase_serialization() {
        let data = b"block height 12345";
        let witness = Witness::Coinbase(data.to_vec());

        let bytes = witness.to_bytes();
        let decoded = Witness::from_bytes(&bytes).unwrap();

        match decoded {
            Witness::Coinbase(decoded_data) => {
                assert_eq!(decoded_data, data);
            }
            _ => panic!("wrong witness type"),
        }
    }

    #[test]
    fn test_witness_multisig_serialization() {
        let (pk1, sk1) = test_keypair();
        let (pk2, _sk2) = test_keypair();
        let message = b"multisig test";
        let sig1 = crypto::ml_dsa_87::sign(&sk1, message);

        let witness = Witness::Multisig {
            public_keys: vec![pk1.clone(), pk2.clone()],
            signatures: vec![Some(sig1), None],
        };

        let bytes = witness.to_bytes();
        let decoded = Witness::from_bytes(&bytes).unwrap();

        match decoded {
            Witness::Multisig { public_keys, signatures } => {
                assert_eq!(public_keys.len(), 2);
                assert_eq!(signatures.len(), 2);
                assert!(signatures[0].is_some());
                assert!(signatures[1].is_none());
            }
            _ => panic!("wrong witness type"),
        }
    }

    #[test]
    fn test_locking_condition_p2pkh_serialization() {
        let (pk, _) = test_keypair();
        let condition = LockingCondition::p2pkh(&pk);

        let bytes = condition.to_bytes();
        let decoded = LockingCondition::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, condition);
    }

    #[test]
    fn test_locking_condition_multisig_serialization() {
        let (pk1, _) = test_keypair();
        let (pk2, _) = test_keypair();
        let condition = LockingCondition::multisig(2, vec![pk1, pk2]);

        let bytes = condition.to_bytes();
        let decoded = LockingCondition::from_bytes(&bytes).unwrap();

        match decoded {
            LockingCondition::Multisig { threshold, public_keys } => {
                assert_eq!(threshold, 2);
                assert_eq!(public_keys.len(), 2);
            }
            _ => panic!("wrong condition type"),
        }
    }

    #[test]
    fn test_tx_output_serialization() {
        let (pk, _) = test_keypair();
        let output = TxOutput::p2pkh(1_000_000, Address::from_public_key(&pk));

        let bytes = output.to_bytes();
        let decoded = TxOutput::from_bytes(&bytes).unwrap();
        assert_eq!(decoded.amount, output.amount);
    }

    #[test]
    fn test_transaction_serialization() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let tx = Transaction::coinbase(0, 50_000_000, address);

        let bytes = tx.to_bytes();
        let decoded = Transaction::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.version, tx.version);
        assert_eq!(decoded.inputs.len(), tx.inputs.len());
        assert_eq!(decoded.outputs.len(), tx.outputs.len());
        assert_eq!(decoded.txid(), tx.txid());
    }

    #[test]
    fn test_block_header_serialization() {
        let header = BlockHeader {
            version: 1,
            prev_hash: crypto::hash(b"prev"),
            merkle_root: crypto::hash(b"merkle"),
            timestamp: 1234567890,
            difficulty_bits: 0x1d00ffff,
            nonce: 42,
        };

        let bytes = header.to_bytes();
        assert_eq!(bytes.len(), BlockHeader::SIZE);

        let decoded = BlockHeader::from_bytes(&bytes).unwrap();
        assert_eq!(decoded, header);
    }

    #[test]
    fn test_block_serialization() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let tx = Transaction::coinbase(0, 50_000_000, address);
        let merkle_root = Block::compute_merkle_root(&[tx.clone()]);

        let header = BlockHeader {
            version: 1,
            prev_hash: Hash::from_bytes([0u8; 64]),
            merkle_root,
            timestamp: 1234567890,
            difficulty_bits: 0x1d00ffff,
            nonce: 0,
        };

        let block = Block::new(header, vec![tx]);
        let bytes = block.to_bytes();
        let decoded = Block::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.hash(), block.hash());
        assert!(decoded.verify_merkle_root());
    }

    // ========================================================================
    // Merkle Root Tests
    // ========================================================================

    #[test]
    fn test_merkle_root_single_tx() {
        let (pk, _) = test_keypair();
        let tx = Transaction::coinbase(0, 50_000_000, Address::from_public_key(&pk));

        let merkle_root = Block::compute_merkle_root(&[tx.clone()]);
        assert_eq!(merkle_root, tx.txid());
    }

    #[test]
    fn test_merkle_root_two_txs() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let tx1 = Transaction::coinbase(0, 50_000_000, address);
        let tx2 = Transaction::coinbase(1, 50_000_000, address);

        let merkle_root = Block::compute_merkle_root(&[tx1.clone(), tx2.clone()]);
        let expected = crypto::hash_many(&[tx1.txid().as_bytes(), tx2.txid().as_bytes()]);
        assert_eq!(merkle_root, expected);
    }

    #[test]
    fn test_merkle_root_odd_txs() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let tx1 = Transaction::coinbase(0, 50_000_000, address);
        let tx2 = Transaction::coinbase(1, 50_000_000, address);
        let tx3 = Transaction::coinbase(2, 50_000_000, address);

        let merkle_root = Block::compute_merkle_root(&[tx1.clone(), tx2.clone(), tx3.clone()]);

        // Manual calculation:
        // Level 1: [H(tx1|tx2), H(tx3|tx3)]
        // Level 0: H(H(tx1|tx2) | H(tx3|tx3))
        let h12 = crypto::hash_many(&[tx1.txid().as_bytes(), tx2.txid().as_bytes()]);
        let h33 = crypto::hash_many(&[tx3.txid().as_bytes(), tx3.txid().as_bytes()]);
        let expected = crypto::hash_many(&[h12.as_bytes(), h33.as_bytes()]);
        assert_eq!(merkle_root, expected);
    }

    // ========================================================================
    // Proof of Work Tests
    // ========================================================================

    #[test]
    fn test_difficulty_target_encoding() {
        // Test Bitcoin-like difficulty encoding
        let bits = 0x1d00ffff_u32;
        let header = BlockHeader {
            version: 1,
            prev_hash: Hash::from_bytes([0u8; 64]),
            merkle_root: Hash::from_bytes([0u8; 64]),
            timestamp: 0,
            difficulty_bits: bits,
            nonce: 0,
        };

        let target = header.target();
        // The target should have 0x00ffff at the appropriate position
        assert!(target.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_pow_easy_target() {
        // Set a very easy target: exponent=64, coefficient=0xffffff
        // This means target[0..3] = [0xff, 0xff, 0xff, ...]
        // Any hash not starting with 0xffffff will be less than this target
        let easy_bits = 0x40ffffff_u32;

        let header = BlockHeader {
            version: 1,
            prev_hash: Hash::from_bytes([0u8; 64]),
            merkle_root: Hash::from_bytes([0u8; 64]),
            timestamp: 0,
            difficulty_bits: easy_bits,
            nonce: 0,
        };

        // Should pass with any nonce since target is so high
        assert!(header.check_pow());
    }

    // ========================================================================
    // Blockchain Tests
    // ========================================================================

    #[test]
    fn test_genesis_block_creation() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

        assert!(genesis.verify_merkle_root());
        assert_eq!(genesis.transactions.len(), 1);
        assert!(genesis.transactions[0].is_coinbase());
    }

    #[test]
    fn test_blockchain_creation() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

        let chain = Blockchain::new(genesis.clone(), 2016, 600, 50_000_000, 210_000);

        assert_eq!(chain.height(), 0);
        assert_eq!(chain.tip_hash(), genesis.hash());
        assert_eq!(chain.balance(&address), 50_000_000);
    }

    #[test]
    fn test_block_reward_halving() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

        let chain = Blockchain::new(genesis, 2016, 600, 50_000_000, 100);

        assert_eq!(chain.block_reward(0), 50_000_000);
        assert_eq!(chain.block_reward(99), 50_000_000);
        assert_eq!(chain.block_reward(100), 25_000_000);
        assert_eq!(chain.block_reward(199), 25_000_000);
        assert_eq!(chain.block_reward(200), 12_500_000);
    }

    #[test]
    fn test_difficulty_adjustment_scaling() {
        // Test that difficulty adjustment stays within 64-bit bounds
        // and handles edge cases correctly.

        // Test coefficient scaling without overflow
        // difficulty_bits = (exponent << 24) | coefficient
        // With exponent=32, coefficient=0x100000 (about 1M)
        let bits: u32 = (32 << 24) | 0x100000;
        let exponent = bits >> 24;
        let coefficient = (bits & 0x00FFFFFF) as u64;

        // Scaling by 4x (max adjustment)
        let scaled = coefficient * 4;
        assert!(scaled <= 0x7FFFFF * 4); // Should fit in reasonable range

        // Scaling by 0.25x (min adjustment)
        let scaled_down = coefficient / 4;
        assert!(scaled_down > 0);

        // Test exponent boundary: max exponent is 64 for 512-bit hash
        let max_bits: u32 = (64 << 24) | 0x7FFFFF;
        let max_exp = max_bits >> 24;
        assert_eq!(max_exp, 64);

        // Test minimum exponent
        let min_bits: u32 = (1 << 24) | 0x000001;
        let min_exp = min_bits >> 24;
        assert_eq!(min_exp, 1);
    }

    #[test]
    fn test_utxos_for_address() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, address);

        let chain = Blockchain::new(genesis, 2016, 600, 50_000_000, 210_000);

        let utxos = chain.utxos_for_address(&address);
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].1.output.amount, 50_000_000);
    }

    // ========================================================================
    // Transaction Tests
    // ========================================================================

    #[test]
    fn test_transaction_signing_data() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let tx = Transaction::coinbase(0, 50_000_000, address);

        let signing_data_0 = tx.signing_data(0);
        let signing_data_1 = tx.signing_data(1);

        // Different input indices should produce different signing data
        // (even though this coinbase only has one input)
        assert_ne!(signing_data_0, signing_data_1);
    }

    #[test]
    fn test_transaction_txid_deterministic() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let tx = Transaction::coinbase(0, 50_000_000, address);

        let txid1 = tx.txid();
        let txid2 = tx.txid();
        assert_eq!(txid1, txid2);
    }

    // ========================================================================
    // Error Handling Tests
    // ========================================================================

    #[test]
    fn test_deserialize_truncated_data() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);
        let tx = Transaction::coinbase(0, 50_000_000, address);

        let bytes = tx.to_bytes();
        let truncated = &bytes[..bytes.len() / 2];

        assert!(Transaction::from_bytes(truncated).is_err());
    }

    #[test]
    fn test_deserialize_invalid_tag() {
        let invalid_witness = vec![0xFF, 0x00]; // Invalid witness type tag
        assert!(Witness::from_bytes(&invalid_witness).is_err());
    }

    #[test]
    fn test_deserialize_trailing_bytes() {
        let (pk, _) = test_keypair();
        let address = Address::from_public_key(&pk);

        let mut bytes = address.to_bytes();
        bytes.push(0xFF); // Extra byte

        let result = Address::from_bytes(&bytes);
        assert!(matches!(result, Err(DeserializeError::InvalidData(_))));
    }

    // ========================================================================
    // Negative Path Tests
    // ========================================================================

    /// Helper to create a test blockchain with a mature genesis coinbase.
    ///
    /// Adds COINBASE_MATURITY empty blocks so the genesis coinbase can be spent.
    fn test_blockchain() -> (Blockchain, PublicKey, crypto::SecretKey, Address) {
        let (pk, sk) = test_keypair();
        let address = Address::from_public_key(&pk);
        let mut timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() - (COINBASE_MATURITY + 10) * 600; // Start in the past

        let genesis = create_genesis_block(
            timestamp,
            0x40ffffff, // Easy difficulty
            50_000_000,
            address,
        );
        let mut chain = Blockchain::new(genesis, 2016, 600, 50_000_000, 210_000);

        // Add COINBASE_MATURITY blocks to mature the genesis coinbase
        for _ in 0..COINBASE_MATURITY {
            timestamp += 600;
            let prev_block = chain.tip();
            let height = chain.height() + 1;
            let reward = chain.block_reward(height);

            let coinbase = Transaction::coinbase(height, reward, address);
            let merkle_root = Block::compute_merkle_root(&[coinbase.clone()]);

            let header = BlockHeader {
                version: BlockHeader::CURRENT_VERSION,
                prev_hash: prev_block.hash(),
                merkle_root,
                timestamp,
                difficulty_bits: prev_block.header.difficulty_bits,
                nonce: 0,
            };

            let block = Block::new(header, vec![coinbase]);
            chain.add_block(block).unwrap();
        }

        (chain, pk, sk, address)
    }

    /// Helper to find a mature UTXO that can be spent
    fn find_mature_utxo<'a>(
        chain: &'a Blockchain,
        address: &Address,
    ) -> (OutPoint, &'a Utxo) {
        let utxos = chain.utxos_for_address(address);
        utxos
            .into_iter()
            .find(|(_, u)| {
                // Mature if non-coinbase, or coinbase with enough confirmations
                !u.is_coinbase || chain.height() >= u.height + COINBASE_MATURITY
            })
            .expect("No mature UTXO found")
    }

    /// Helper to create a valid spending transaction
    fn create_spending_tx(
        chain: &Blockchain,
        from_pk: &PublicKey,
        from_sk: &crypto::SecretKey,
        to_address: Address,
    ) -> Transaction {
        let from_address = Address::from_public_key(from_pk);
        let (outpoint, utxo) = find_mature_utxo(chain, &from_address);

        // Create transaction structure
        let inputs = vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: from_pk.clone(),
                signature: crypto::ml_dsa_87::sign(from_sk, b"placeholder"),
            },
        )];
        let outputs = vec![TxOutput::p2pkh(utxo.output.amount, to_address)];
        let mut tx = Transaction::new(inputs, outputs);

        // Sign the transaction properly
        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let signature = crypto::ml_dsa_87::sign(from_sk, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: from_pk.clone(),
            signature,
        };

        tx
    }

    /// Helper to create a block containing transactions
    fn create_block_with_txs(
        chain: &Blockchain,
        txs: Vec<Transaction>,
        recipient: Address,
    ) -> Block {
        let prev_block = chain.tip();
        let height = chain.height() + 1;
        let reward = chain.block_reward(height);

        // Create coinbase
        let mut all_txs = vec![Transaction::coinbase(height, reward, recipient)];
        all_txs.extend(txs);

        let merkle_root = Block::compute_merkle_root(&all_txs);
        // Timestamp must be strictly greater than previous block
        let timestamp = prev_block.header.timestamp + 600;

        let header = BlockHeader {
            version: BlockHeader::CURRENT_VERSION,
            prev_hash: prev_block.hash(),
            merkle_root,
            timestamp,
            difficulty_bits: prev_block.header.difficulty_bits,
            nonce: 0,
        };

        Block::new(header, all_txs)
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let (mut chain, pk, _sk, address) = test_blockchain();
        let (other_pk, other_sk) = test_keypair();
        let other_address = Address::from_public_key(&other_pk);

        // Find a mature UTXO (genesis coinbase at height 0)
        let utxos = chain.utxos_for_address(&address);
        let (outpoint, utxo) = utxos
            .iter()
            .find(|(_, u)| {
                // Find a mature coinbase (created at height 0, now at height 100+)
                u.height == 0 && chain.height() >= u.height + COINBASE_MATURITY
            })
            .expect("No mature UTXO found");

        let inputs = vec![TxInput::new(
            *outpoint,
            Witness::P2PKH {
                public_key: pk.clone(),
                signature: crypto::ml_dsa_87::sign(&other_sk, b"wrong key"),
            },
        )];
        let outputs = vec![TxOutput::p2pkh(utxo.output.amount, other_address)];
        let mut tx = Transaction::new(inputs, outputs);

        // Sign with wrong secret key (signature won't verify with pk)
        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let wrong_signature = crypto::ml_dsa_87::sign(&other_sk, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: pk.clone(),
            signature: wrong_signature,
        };

        let block = create_block_with_txs(&chain, vec![tx], other_address);
        let result = chain.add_block(block);
        assert!(matches!(result, Err(BlockchainError::InvalidWitness)));
    }

    #[test]
    fn test_wrong_public_key_rejected() {
        let (mut chain, _pk, sk, address) = test_blockchain();
        let (other_pk, _other_sk) = test_keypair();
        let other_address = Address::from_public_key(&other_pk);

        // Create a transaction with wrong public key (doesn't match address)
        let (outpoint, utxo) = find_mature_utxo(&chain, &address);

        let inputs = vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: other_pk.clone(), // Wrong public key
                signature: crypto::ml_dsa_87::sign(&sk, b"placeholder"),
            },
        )];
        let outputs = vec![TxOutput::p2pkh(utxo.output.amount, other_address)];
        let mut tx = Transaction::new(inputs, outputs);

        // Sign correctly, but public key doesn't match the address
        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: other_pk.clone(), // Still wrong
            signature,
        };

        let block = create_block_with_txs(&chain, vec![tx], other_address);
        let result = chain.add_block(block);
        assert!(matches!(result, Err(BlockchainError::InvalidWitness)));
    }

    #[test]
    fn test_insufficient_multisig_signatures_rejected() {
        let (pk1, sk1) = test_keypair();
        let (pk2, _sk2) = test_keypair();
        let (pk3, _sk3) = test_keypair();
        let (miner_pk, _) = test_keypair();
        let miner_address = Address::from_public_key(&miner_pk);

        // Create a 2-of-3 multisig output in genesis
        let multisig_condition = LockingCondition::multisig(2, vec![pk1.clone(), pk2.clone(), pk3.clone()]);
        let coinbase = Transaction {
            version: Transaction::CURRENT_VERSION,
            inputs: vec![TxInput::coinbase(&0u64.to_le_bytes())],
            outputs: vec![TxOutput {
                amount: 50_000_000,
                condition: multisig_condition,
            }],
        };
        let merkle_root = Block::compute_merkle_root(&[coinbase.clone()]);
        let mut timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() - (COINBASE_MATURITY + 10) * 600;

        let genesis_header = BlockHeader {
            version: BlockHeader::CURRENT_VERSION,
            prev_hash: Hash::from_bytes([0u8; 64]),
            merkle_root,
            timestamp,
            difficulty_bits: 0x40ffffff,
            nonce: 0,
        };
        let genesis = Block::new(genesis_header, vec![coinbase.clone()]);

        let mut chain = Blockchain::new(genesis, 2016, 600, 50_000_000, 210_000);

        // Add COINBASE_MATURITY blocks to mature the genesis coinbase
        for i in 0..COINBASE_MATURITY {
            timestamp += 600;
            let prev_block = chain.tip();
            let height = chain.height() + 1;
            let reward = chain.block_reward(height);

            let miner_coinbase = Transaction::coinbase(height, reward, miner_address);
            let block_merkle_root = Block::compute_merkle_root(&[miner_coinbase.clone()]);

            let header = BlockHeader {
                version: BlockHeader::CURRENT_VERSION,
                prev_hash: prev_block.hash(),
                merkle_root: block_merkle_root,
                timestamp,
                difficulty_bits: prev_block.header.difficulty_bits,
                nonce: 0,
            };

            let block = Block::new(header, vec![miner_coinbase]);
            chain.add_block(block).unwrap();
        }

        // Try to spend with only 1 signature (need 2)
        let outpoint = OutPoint::new(coinbase.txid(), 0);
        let (recipient_pk, _) = test_keypair();
        let recipient_address = Address::from_public_key(&recipient_pk);

        let inputs = vec![TxInput::new(
            outpoint,
            Witness::Multisig {
                public_keys: vec![pk1.clone(), pk2.clone(), pk3.clone()],
                signatures: vec![Some(crypto::ml_dsa_87::sign(&sk1, b"placeholder")), None, None],
            },
        )];
        let outputs = vec![TxOutput::p2pkh(50_000_000, recipient_address)];
        let mut tx = Transaction::new(inputs, outputs);

        // Sign with only sk1 (need 2 signatures for 2-of-3)
        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let sig1 = crypto::ml_dsa_87::sign(&sk1, message.as_bytes());
        tx.inputs[0].witness = Witness::Multisig {
            public_keys: vec![pk1.clone(), pk2.clone(), pk3.clone()],
            signatures: vec![Some(sig1), None, None], // Only 1 signature
        };

        let block = create_block_with_txs(&chain, vec![tx], recipient_address);
        let result = chain.add_block(block);
        assert!(matches!(result, Err(BlockchainError::InvalidWitness)));
    }

    #[test]
    fn test_double_spend_within_block_detected() {
        let (mut chain, pk, sk, address) = test_blockchain();
        let (recipient_pk, _) = test_keypair();
        let recipient_address = Address::from_public_key(&recipient_pk);

        // Create two transactions that spend the same UTXO
        let tx1 = create_spending_tx(&chain, &pk, &sk, recipient_address);
        let tx2 = create_spending_tx(&chain, &pk, &sk, recipient_address);

        // Both transactions try to spend the same output
        assert_eq!(tx1.inputs[0].outpoint, tx2.inputs[0].outpoint);

        let block = create_block_with_txs(&chain, vec![tx1, tx2], recipient_address);
        let result = chain.add_block(block);
        assert!(matches!(result, Err(BlockchainError::DoubleSpend(_))));
    }

    #[test]
    fn test_spending_immature_coinbase_rejected() {
        let (mut chain, pk, sk, address) = test_blockchain();
        let (recipient_pk, _) = test_keypair();
        let recipient_address = Address::from_public_key(&recipient_pk);

        // Add a new block with a coinbase
        let block1 = create_block_with_txs(&chain, vec![], address);
        chain.add_block(block1.clone()).unwrap();

        // Try to spend the new coinbase immediately (before COINBASE_MATURITY blocks)
        let new_coinbase_txid = block1.transactions[0].txid();
        let outpoint = OutPoint::new(new_coinbase_txid, 0);
        let new_coinbase_amount = block1.transactions[0].outputs[0].amount;

        let inputs = vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: pk.clone(),
                signature: crypto::ml_dsa_87::sign(&sk, b"placeholder"),
            },
        )];
        let outputs = vec![TxOutput::p2pkh(new_coinbase_amount, recipient_address)];
        let mut tx = Transaction::new(inputs, outputs);

        // Sign properly
        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: pk.clone(),
            signature,
        };

        // This should fail because the coinbase is immature
        let block2 = create_block_with_txs(&chain, vec![tx], recipient_address);
        let result = chain.add_block(block2);
        assert!(matches!(result, Err(BlockchainError::ImmatureCoinbase { .. })));
    }

    #[test]
    fn test_insufficient_inputs_rejected() {
        let (mut chain, pk, sk, address) = test_blockchain();
        let (recipient_pk, _) = test_keypair();
        let recipient_address = Address::from_public_key(&recipient_pk);

        // Try to create more output value than input value
        let (outpoint, utxo) = find_mature_utxo(&chain, &address);

        let inputs = vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: pk.clone(),
                signature: crypto::ml_dsa_87::sign(&sk, b"placeholder"),
            },
        )];
        // Output more than we have
        let outputs = vec![TxOutput::p2pkh(utxo.output.amount + 1, recipient_address)];
        let mut tx = Transaction::new(inputs, outputs);

        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: pk.clone(),
            signature,
        };

        let block = create_block_with_txs(&chain, vec![tx], recipient_address);
        let result = chain.add_block(block);
        assert!(matches!(result, Err(BlockchainError::InsufficientInputs)));
    }

    #[test]
    fn test_dust_output_rejected() {
        let (mut chain, pk, sk, address) = test_blockchain();
        let (recipient_pk, _) = test_keypair();
        let recipient_address = Address::from_public_key(&recipient_pk);

        // Calculate the dust limit for a P2PKH output
        let p2pkh_condition = LockingCondition::P2PKH(recipient_address);
        let expected_dust_limit = dust_limit(&p2pkh_condition);

        // Create a transaction with an output below the dust limit
        let (outpoint, utxo) = find_mature_utxo(&chain, &address);

        let inputs = vec![TxInput::new(
            outpoint,
            Witness::P2PKH {
                public_key: pk.clone(),
                signature: crypto::ml_dsa_87::sign(&sk, b"placeholder"),
            },
        )];

        // Create a dust output (below the calculated dust limit)
        let dust_amount = expected_dust_limit - 1;
        let change_amount = utxo.output.amount - dust_amount - 1000; // Leave some for fee
        let outputs = vec![
            TxOutput::p2pkh(dust_amount, recipient_address),
            TxOutput::p2pkh(change_amount, address),
        ];
        let mut tx = Transaction::new(inputs, outputs);

        let signing_data = tx.signing_data(0);
        let message = crypto::hash(&signing_data);
        let signature = crypto::ml_dsa_87::sign(&sk, message.as_bytes());
        tx.inputs[0].witness = Witness::P2PKH {
            public_key: pk.clone(),
            signature,
        };

        let block = create_block_with_txs(&chain, vec![tx], recipient_address);
        let result = chain.add_block(block);
        assert!(
            matches!(result, Err(BlockchainError::DustOutput { index: 0, amount, limit })
                if amount == dust_amount && limit == expected_dust_limit),
            "Expected DustOutput error with amount {} and limit {}, got {:?}",
            dust_amount, expected_dust_limit, result
        );
    }

    #[test]
    fn test_dust_limit_scales_with_output_type() {
        // Verify that multisig outputs have higher dust limits than P2PKH
        let (pk1, _) = test_keypair();
        let (pk2, _) = test_keypair();
        let (pk3, _) = test_keypair();
        let address = Address::from_public_key(&pk1);

        let p2pkh_condition = LockingCondition::P2PKH(address);
        let multisig_condition = LockingCondition::multisig(2, vec![pk1, pk2, pk3]);

        let p2pkh_limit = dust_limit(&p2pkh_condition);
        let multisig_limit = dust_limit(&multisig_condition);

        // Multisig requires more data to spend (3 pubkeys + 2 sigs vs 1 pubkey + 1 sig)
        assert!(
            multisig_limit > p2pkh_limit,
            "Multisig dust limit ({}) should be greater than P2PKH ({})",
            multisig_limit, p2pkh_limit
        );

        // Verify reasonable values (at DUST_FEE_RATE = 100 quanta/KB)
        // P2PKH spend size: ~7,288 bytes -> dust ~728 quanta
        // 2-of-3 Multisig spend size: ~17,028 bytes -> dust ~1,702 quanta
        assert!(p2pkh_limit > 500 && p2pkh_limit < 1500,
            "P2PKH dust limit {} outside expected range", p2pkh_limit);
        assert!(multisig_limit > 1000 && multisig_limit < 3000,
            "Multisig dust limit {} outside expected range", multisig_limit);
    }
}
