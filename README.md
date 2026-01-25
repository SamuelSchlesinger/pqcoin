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
```

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
listen_port = 8333
max_peers = 125

[mining]
enabled = true
threads = 4

[logging]
level = "info"
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

## Documentation

See [whitepaper/pqcoin.tex](whitepaper/pqcoin.tex) for the full technical specification.

## License

MIT
