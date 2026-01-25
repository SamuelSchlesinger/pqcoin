//! Heed codecs for blockchain types.
//!
//! These codecs enable storing blockchain types in LMDB via the heed crate.
//! We leverage the existing `Serialize`/`Deserialize` traits for types that
//! have variable-length encoding, and use zero-copy encoding for fixed-size types.

use std::borrow::Cow;
use std::marker::PhantomData;

use heed::{BoxedError, BytesDecode, BytesEncode};

use crate::blockchain::{Block, Deserialize, OutPoint, Serialize, Utxo};
use crate::crypto::Hash;

/// Codec for `Hash` (64 bytes, fixed size).
///
/// Uses zero-copy encoding since Hash is always exactly 64 bytes.
pub struct HashCodec;

impl<'a> BytesEncode<'a> for HashCodec {
    type EItem = Hash;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        Ok(Cow::Borrowed(item.as_bytes()))
    }
}

impl<'a> BytesDecode<'a> for HashCodec {
    type DItem = Hash;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if bytes.len() != Hash::SIZE {
            return Err(format!(
                "invalid hash length: expected {}, got {}",
                Hash::SIZE,
                bytes.len()
            )
            .into());
        }
        let mut arr = [0u8; 64];
        arr.copy_from_slice(bytes);
        Ok(Hash::from_bytes(arr))
    }
}

/// Codec for `OutPoint` (68 bytes: 64-byte txid + 4-byte index).
///
/// Uses the existing `Serialize`/`Deserialize` implementation.
pub struct OutPointCodec;

impl<'a> BytesEncode<'a> for OutPointCodec {
    type EItem = OutPoint;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        Ok(Cow::Owned(item.to_bytes()))
    }
}

impl<'a> BytesDecode<'a> for OutPointCodec {
    type DItem = OutPoint;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        OutPoint::from_bytes(bytes).map_err(|e| format!("failed to decode OutPoint: {}", e).into())
    }
}

/// Generic codec for types implementing the blockchain `Serialize`/`Deserialize` traits.
///
/// This is used for `Block` and `Utxo` which have variable-length encodings.
pub struct BlockchainCodec<T>(PhantomData<T>);

impl<'a, T: Serialize + 'a> BytesEncode<'a> for BlockchainCodec<T> {
    type EItem = T;

    fn bytes_encode(item: &Self::EItem) -> Result<Cow<'_, [u8]>, BoxedError> {
        Ok(Cow::Owned(item.to_bytes()))
    }
}

impl<'a, T: Deserialize + 'static> BytesDecode<'a> for BlockchainCodec<T> {
    type DItem = T;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        T::from_bytes(bytes).map_err(|e| format!("failed to decode: {}", e).into())
    }
}

/// Type alias for Block codec.
pub type BlockCodec = BlockchainCodec<Block>;

/// Type alias for Utxo codec.
pub type UtxoCodec = BlockchainCodec<Utxo>;

/// Codec for u64 values (8 bytes, big-endian for proper ordering).
///
/// We use big-endian encoding so that LMDB's lexicographic ordering
/// matches numeric ordering.
pub struct U64Codec;

impl<'a> BytesEncode<'a> for U64Codec {
    type EItem = u64;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        Ok(Cow::Owned(item.to_be_bytes().to_vec()))
    }
}

impl<'a> BytesDecode<'a> for U64Codec {
    type DItem = u64;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        if bytes.len() != 8 {
            return Err(format!("invalid u64 length: expected 8, got {}", bytes.len()).into());
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(arr))
    }
}

/// Codec for string keys (metadata).
pub struct StrCodec;

impl<'a> BytesEncode<'a> for StrCodec {
    type EItem = str;

    fn bytes_encode(item: &'a Self::EItem) -> Result<Cow<'a, [u8]>, BoxedError> {
        Ok(Cow::Borrowed(item.as_bytes()))
    }
}

impl<'a> BytesDecode<'a> for StrCodec {
    type DItem = String;

    fn bytes_decode(bytes: &'a [u8]) -> Result<Self::DItem, BoxedError> {
        String::from_utf8(bytes.to_vec()).map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::{Address, LockingCondition, TxOutput};

    #[test]
    fn test_hash_codec_roundtrip() {
        let hash = crate::crypto::hash(b"test data");
        let encoded = HashCodec::bytes_encode(&hash).unwrap();
        let decoded = HashCodec::bytes_decode(&encoded).unwrap();
        assert_eq!(hash, decoded);
    }

    #[test]
    fn test_outpoint_codec_roundtrip() {
        let hash = crate::crypto::hash(b"txid");
        let outpoint = OutPoint::new(hash, 42);
        let encoded = OutPointCodec::bytes_encode(&outpoint).unwrap();
        let decoded = OutPointCodec::bytes_decode(&encoded).unwrap();
        assert_eq!(outpoint, decoded);
    }

    #[test]
    fn test_u64_codec_roundtrip() {
        for value in [0u64, 1, 100, u64::MAX / 2, u64::MAX] {
            let encoded = U64Codec::bytes_encode(&value).unwrap();
            let decoded = U64Codec::bytes_decode(&encoded).unwrap();
            assert_eq!(value, decoded);
        }
    }

    #[test]
    fn test_u64_codec_ordering() {
        // Big-endian encoding should preserve numeric ordering
        let values = [0u64, 1, 100, 1000, u64::MAX];
        let mut encoded: Vec<_> = values
            .iter()
            .map(|v| U64Codec::bytes_encode(v).unwrap().to_vec())
            .collect();
        encoded.sort();
        let decoded: Vec<_> = encoded
            .iter()
            .map(|e| U64Codec::bytes_decode(e).unwrap())
            .collect();
        assert_eq!(decoded, values.to_vec());
    }

    #[test]
    fn test_utxo_codec_roundtrip() {
        let address = Address::from_hash(crate::crypto::hash(b"test address"));
        let utxo = Utxo {
            output: TxOutput {
                amount: 50_000_000,
                condition: LockingCondition::P2PKH(address),
            },
            height: 100,
            is_coinbase: true,
        };
        let encoded = UtxoCodec::bytes_encode(&utxo).unwrap();
        let decoded = UtxoCodec::bytes_decode(&encoded).unwrap();
        assert_eq!(utxo.output.amount, decoded.output.amount);
        assert_eq!(utxo.height, decoded.height);
        assert_eq!(utxo.is_coinbase, decoded.is_coinbase);
    }

    #[test]
    fn test_str_codec_roundtrip() {
        let s = "tip_hash";
        let encoded = StrCodec::bytes_encode(s).unwrap();
        let decoded = StrCodec::bytes_decode(&encoded).unwrap();
        assert_eq!(s, decoded);
    }
}
