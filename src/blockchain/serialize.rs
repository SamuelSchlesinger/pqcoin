//! Serialization traits and helpers for blockchain data structures.
//!
//! All structures implement deterministic binary serialization via the [`Serialize`] and
//! [`Deserialize`] traits. The format is compact and uses little-endian byte order for
//! multi-byte integers.

use crate::constants::MAX_SERIALIZE_BYTES;

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
pub(crate) fn write_u8(buf: &mut Vec<u8>, value: u8) {
    buf.push(value);
}

pub(crate) fn write_u32(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn write_u64(buf: &mut Vec<u8>, value: u64) {
    buf.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn write_var_int(buf: &mut Vec<u8>, value: u64) {
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

pub(crate) fn write_bytes(buf: &mut Vec<u8>, data: &[u8]) {
    write_var_int(buf, data.len() as u64);
    buf.extend_from_slice(data);
}

// Helper functions for deserialization
pub(crate) fn read_u8(data: &[u8]) -> Result<(u8, &[u8]), DeserializeError> {
    if data.is_empty() {
        return Err(DeserializeError::UnexpectedEof);
    }
    Ok((data[0], &data[1..]))
}

pub(crate) fn read_u32(data: &[u8]) -> Result<(u32, &[u8]), DeserializeError> {
    if data.len() < 4 {
        return Err(DeserializeError::UnexpectedEof);
    }
    let value = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    Ok((value, &data[4..]))
}

pub(crate) fn read_u64(data: &[u8]) -> Result<(u64, &[u8]), DeserializeError> {
    if data.len() < 8 {
        return Err(DeserializeError::UnexpectedEof);
    }
    let value = u64::from_le_bytes([
        data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
    ]);
    Ok((value, &data[8..]))
}

pub(crate) fn read_var_int(data: &[u8]) -> Result<(u64, &[u8]), DeserializeError> {
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

pub(crate) fn read_bytes(data: &[u8]) -> Result<(Vec<u8>, &[u8]), DeserializeError> {
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

pub(crate) fn read_fixed_bytes<const N: usize>(data: &[u8]) -> Result<([u8; N], &[u8]), DeserializeError> {
    if data.len() < N {
        return Err(DeserializeError::UnexpectedEof);
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&data[..N]);
    Ok((arr, &data[N..]))
}
