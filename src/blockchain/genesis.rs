//! Genesis block creation.

use super::address::Address;
use super::block::Block;
use super::header::BlockHeader;
use super::transaction::Transaction;
use crate::crypto::Hash;

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
    let merkle_root = Block::compute_merkle_root(std::slice::from_ref(&coinbase));

    let header = BlockHeader {
        version: BlockHeader::CURRENT_VERSION,
        prev_hash: Hash::from_bytes([0u8; 64]),
        merkle_root,
        timestamp,
        difficulty_bits,
        nonce: [0u8; 32],
    };

    Block::new(header, vec![coinbase])
}
