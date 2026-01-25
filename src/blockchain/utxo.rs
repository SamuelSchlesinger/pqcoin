//! UTXO (Unspent Transaction Output) type.

use super::output::TxOutput;
use super::serialize::{
    Deserialize, DeserializeError, Serialize, read_u8, read_u64, write_u8, write_u64,
};

/// An unspent transaction output in the UTXO set.
///
/// # Serialization Format
///
/// | Field       | Size     | Description                           |
/// |-------------|----------|---------------------------------------|
/// | output      | variable | The transaction output                |
/// | height      | 8 bytes  | Block height (little-endian u64)      |
/// | is_coinbase | 1 byte   | 1 if coinbase, 0 otherwise            |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Utxo {
    /// The output data.
    pub output: TxOutput,
    /// The height at which this output was created.
    pub height: u64,
    /// Whether this is from a coinbase transaction.
    pub is_coinbase: bool,
}

impl Serialize for Utxo {
    fn serialize(&self, buf: &mut Vec<u8>) {
        self.output.serialize(buf);
        write_u64(buf, self.height);
        write_u8(buf, if self.is_coinbase { 1 } else { 0 });
    }
}

impl Deserialize for Utxo {
    fn deserialize(data: &[u8]) -> Result<(Self, &[u8]), DeserializeError> {
        let (output, data) = TxOutput::deserialize(data)?;
        let (height, data) = read_u64(data)?;
        let (is_coinbase_byte, data) = read_u8(data)?;
        let is_coinbase = is_coinbase_byte != 0;
        Ok((
            Utxo {
                output,
                height,
                is_coinbase,
            },
            data,
        ))
    }
}
