//! # pqcoin - A Post-Quantum Cryptocurrency
//!
//! pqcoin is a proof-of-work cryptocurrency that uses post-quantum cryptographic
//! primitives to provide security against both classical and quantum computers.
//!
//! ## Design Philosophy
//!
//! The design follows Bitcoin's UTXO model with key simplifications:
//!
//! - **No scripting**: Instead of a Turing-incomplete scripting language, pqcoin
//!   supports only two output types: P2PKH (pay-to-public-key-hash) and M-of-N multisig.
//!
//! - **Post-quantum security**: All signatures use ML-DSA-87 (FIPS 204), providing
//!   NIST security level 5 against quantum attacks.
//!
//! - **Modern hashing**: All hashes use SHA3-512 (FIPS 202) for improved security
//!   margins over SHA-256.
//!
//! ## Modules
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`crypto`] | Cryptographic primitives (SHA3-512, ML-DSA-87) |
//! | [`blockchain`] | Core data structures (transactions, blocks, chain state) |
//! | [`mempool`] | Unconfirmed transaction pool with validation |
//! | [`miner`] | Proof-of-work block mining |
//! | [`network`] | P2P networking and block synchronization |
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
//! │   Network   │────▶│  Blockchain │◀────│    Miner    │
//! │   Service   │     │    State    │     │             │
//! └─────────────┘     └─────────────┘     └─────────────┘
//!        │                   ▲                   │
//!        │                   │                   │
//!        ▼                   │                   │
//! ┌─────────────┐            │                   │
//! │   Mempool   │────────────┴───────────────────┘
//! └─────────────┘
//! ```
//!
//! ## Quick Start
//!
//! ```
//! use pqcoin::crypto::ml_dsa_87;
//! use pqcoin::blockchain::{Address, Transaction, Block, Blockchain, create_genesis_block};
//!
//! // Generate a keypair
//! let (public_key, secret_key) = ml_dsa_87::keygen();
//! let address = Address::from_public_key(&public_key);
//!
//! // Create a genesis block
//! let genesis = create_genesis_block(
//!     0,           // timestamp
//!     0x40ffffff,  // easy difficulty for testing
//!     50_000_000,  // initial reward (50 coins)
//!     address,
//! );
//!
//! // Initialize the blockchain
//! let blockchain = Blockchain::new(
//!     genesis,
//!     2016,        // difficulty adjustment interval
//!     600,         // target block time (10 minutes)
//!     50_000_000,  // initial reward
//!     210_000,     // halving interval
//! );
//!
//! println!("Chain height: {}", blockchain.height());
//! println!("Balance: {} quanta", blockchain.balance(&address));
//! ```

pub mod api;
pub mod blockchain;
pub mod config;
pub mod constants;
pub mod crypto;
pub mod mempool;
pub mod miner;
pub mod network;
pub mod wallet;

// Re-export commonly used types for convenience
pub use blockchain::{
    Address, Block, BlockHeader, Blockchain, BlockchainError, DeserializeError, Deserialize,
    LockingCondition, OutPoint, Serialize, Transaction, TxInput, TxOutput, Utxo, Witness,
    create_genesis_block,
};

// Re-export constants
pub use constants::{
    // Protocol constants
    MAX_BLOCK_TXS, MAX_TX_INPUTS, MAX_TX_OUTPUTS, MAX_MULTISIG_KEYS, MAX_SERIALIZE_BYTES,
    MAX_BLOCK_SIZE, COINBASE_MATURITY, DIFFICULTY_COEFFICIENT_MASK, MAX_FUTURE_BLOCK_TIME,
    // Network constants
    DEFAULT_PORT, MAX_PEERS, MAX_OUTBOUND, MAX_ORPHAN_BLOCKS, MAX_BLOCKS_IN_FLIGHT,
    MAX_PENDING_HEADERS, MAX_HEADERS_COUNT, MAX_CONNECTIONS_PER_IP, CONNECTION_RATE_LIMIT_SECS,
    BAN_DURATION_SECS, BAN_SCORE_THRESHOLD,
    // Mining constants
    DEFAULT_DIFFICULTY, INITIAL_REWARD, DIFFICULTY_INTERVAL, TARGET_BLOCK_TIME, HALVING_INTERVAL,
};

pub use crypto::{Hash, PublicKey, SecretKey, Signature, hash, hash_many};

pub use mempool::{Mempool, MempoolError};
pub use miner::{mine_block, BackgroundMiner, MineResult};
