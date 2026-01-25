//! LockingCondition type specifying spending requirements for outputs.

use super::address::Address;
use super::serialize::{
    Deserialize, DeserializeError, Serialize, read_bytes, read_u8, read_var_int, write_bytes,
    write_u8, write_var_int,
};
use crate::constants::MAX_MULTISIG_KEYS;
use crate::crypto::PublicKey;

/// ML-DSA-87 public key size in bytes.
const PQ_PUBLIC_KEY_SIZE: usize = 2592;
/// ML-DSA-87 signature size in bytes.
const PQ_SIGNATURE_SIZE: usize = 4627;

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
        const VARINT_OVERHEAD: usize = 4; // conservative estimate for length prefixes

        match self {
            LockingCondition::P2PKH(_) => {
                // OutPoint + tag(1) + pubkey + signature + varint overhead
                OUTPOINT_SIZE + 1 + PQ_PUBLIC_KEY_SIZE + PQ_SIGNATURE_SIZE + VARINT_OVERHEAD
            }
            LockingCondition::Multisig {
                threshold,
                public_keys,
            } => {
                // OutPoint + tag(1) + all pubkeys + threshold signatures + varint overhead
                let num_keys = public_keys.len();
                let num_sigs = *threshold as usize;
                OUTPOINT_SIZE
                    + 1
                    + (num_keys * PQ_PUBLIC_KEY_SIZE)
                    + (num_sigs * PQ_SIGNATURE_SIZE)
                    + VARINT_OVERHEAD * 2 // for key count and sig count
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
                    let pk = PublicKey::from_bytes(&pk_bytes).ok_or_else(|| {
                        DeserializeError::InvalidData("invalid public key".into())
                    })?;
                    public_keys.push(pk);
                    data = rest;
                }
                Ok((
                    LockingCondition::Multisig {
                        threshold,
                        public_keys,
                    },
                    data,
                ))
            }
            _ => Err(DeserializeError::InvalidData(format!(
                "unknown locking condition type: {tag}"
            ))),
        }
    }
}
