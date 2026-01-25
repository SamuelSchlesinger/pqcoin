//! Witness data proving authorization to spend outputs.

use crate::constants::MAX_MULTISIG_KEYS;
use crate::crypto::{PublicKey, Signature};
use super::serialize::{
    Deserialize, DeserializeError, Serialize,
    read_bytes, read_u8, read_var_int, write_bytes, write_u8, write_var_int,
};

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
