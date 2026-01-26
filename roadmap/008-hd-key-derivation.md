---
title: "HD Key Derivation"
priority: 3
status: planned
tags: [wallet, security]
dependencies: []
---

# HD Key Derivation

Implement hierarchical deterministic key derivation for the wallet.

## Goals

- Single seed phrase backs up unlimited keys
- BIP-32 style derivation paths
- Quantum-safe derivation using SHA3

## Design

Since ML-DSA doesn't have a standard HD derivation scheme, we'll use:

1. Generate master seed from mnemonic (BIP-39 compatible word list)
2. Derive child seeds using SHA3-512(parent_seed || index)
3. Generate ML-DSA keypairs from child seeds

```
master_seed (from mnemonic)
    |
    +-- SHA3-512(seed || "m/44'/pqc'/0'/0/0") --> keypair 0
    +-- SHA3-512(seed || "m/44'/pqc'/0'/0/1") --> keypair 1
    ...
```

## Commands

```bash
# Create wallet with mnemonic
pqwallet create --hd
# Displays: "word1 word2 ... word24"

# Recover wallet from mnemonic
pqwallet recover

# Derive new address
pqwallet address new
```

## Acceptance Criteria

- Wallet generates 24-word mnemonic on creation
- Same mnemonic always produces same addresses
- Documented derivation scheme
