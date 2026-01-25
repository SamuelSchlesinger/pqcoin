# pqcoin

A post-quantum cryptocurrency using NIST-standardized cryptographic primitives.

## Overview

pqcoin is a proof-of-work cryptocurrency designed for the post-quantum era. It uses:

- **ML-DSA-87** (FIPS 204) for digital signatures
- **SHA3-512** (FIPS 202) for hashing

The design follows Bitcoin's proven UTXO model while removing scripting complexity in favor of two well-defined output types: P2PKH and M-of-N multisig.

## Features

- **Quantum resistant**: NIST Level 5 post-quantum signatures
- **Simple transaction model**: No scripting, just P2PKH and multisig
- **256-bit nonce**: Eliminates extraNonce complexity
- **Bitcoin-inspired**: Familiar UTXO model, 10-minute blocks, 21M supply cap
- **Durable storage**: LMDB-based persistence with crash recovery

## Building

```bash
cargo build --release
```

## Binaries

- **pqcoin**: Full node with P2P networking and mining
- **pqwallet**: Command-line wallet

## Running a Node

```bash
# Start a node with mining enabled
./target/release/pqcoin --mine

# With custom configuration
./target/release/pqcoin --config pqcoin.toml

# With custom data directory
./target/release/pqcoin --datadir /path/to/data

# Enable RPC API
./target/release/pqcoin --rpc
```

Blockchain data is persisted to `~/.pqcoin/data` by default. The node will automatically recover its state on restart.

## Wallet Usage

```bash
# Create a new wallet
./target/release/pqwallet create

# Show wallet address
./target/release/pqwallet address

# Check balance
./target/release/pqwallet balance

# Send funds
./target/release/pqwallet send <address> <amount>
```

## Configuration

Configuration can be provided via:
1. Command-line flags (highest priority)
2. `./pqcoin.toml` (local config)
3. `~/.pqcoin/config.toml` (user config)
4. Built-in defaults

Example configuration:

```toml
[network]
port = 8333
max_peers = 125

[mining]
enabled = true

[logging]
level = "info"

[rpc]
enabled = true
port = 8332
bind = "127.0.0.1"

[storage]
# Data directory for blockchain storage (default: ~/.pqcoin/data)
path = "/custom/path/to/data"
```

## Protocol Constants

| Parameter | Value |
|-----------|-------|
| Block time | 10 minutes |
| Difficulty adjustment | Every 2,016 blocks |
| Initial reward | 50 coins |
| Halving interval | 210,000 blocks |
| Max block size | 16 MB |
| Coinbase maturity | 100 blocks |

## Cryptographic Parameters

| Algorithm | Component | Size |
|-----------|-----------|------|
| SHA3-512 | Hash | 64 bytes |
| ML-DSA-87 | Public key | 2,592 bytes |
| ML-DSA-87 | Signature | 4,627 bytes |

## RPC API

The node exposes a JSON-RPC API compatible with common Bitcoin methods:

- `getblockchaininfo` - Chain state
- `getblock` - Block by hash
- `getblockhash` - Hash by height
- `sendrawtransaction` - Broadcast transaction
- `getbalance` - Address balance
- `getutxos` - Unspent outputs

## Architecture

### Storage

The node uses LMDB for durable storage with a hybrid cache + write-through architecture:

- **Reads**: Served from in-memory caches (O(1) HashMap lookups)
- **Writes**: Persisted to LMDB in atomic transactions
- **Startup**: State loaded from LMDB into memory

All block mutations are atomic - if the process crashes mid-write, LMDB automatically rolls back to the last consistent state.

**Database schema:**

| Database | Key | Value | Purpose |
|----------|-----|-------|---------|
| `blocks` | Hash | Block | All blocks by hash |
| `heights` | Hash | u64 | Block hash → height |
| `utxos` | OutPoint | Utxo | Unspent outputs |
| `tx_index` | Hash | Hash | Transaction → block |
| `metadata` | String | bytes | Chain tip, config |

## Documentation

See [whitepaper/pqcoin.tex](whitepaper/pqcoin.tex) for the full technical specification.

## License

MIT
