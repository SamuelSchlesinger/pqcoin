---
title: "Multisig in CLI Wallet"
priority: 3
status: planned
tags: [wallet]
dependencies: []
---

# Multisig in CLI Wallet

Add M-of-N multisig support to the pqwallet CLI.

## Goals

- Create and manage multisig addresses
- Coordinate signature collection for multisig spends
- Support common configurations (2-of-3, 3-of-5)

## Commands

```bash
# Create a multisig address
pqwallet multisig create --required 2 --pubkeys key1.pub,key2.pub,key3.pub

# Export public key for sharing
pqwallet pubkey export --file my.pub

# Sign a multisig transaction (partial)
pqwallet multisig sign --tx unsigned.tx --output partial.tx

# Combine partial signatures
pqwallet multisig combine --partials sig1.tx,sig2.tx --output final.tx

# Broadcast completed transaction
pqwallet send --raw final.tx
```

## Implementation Notes

- Store multisig configurations in wallet file
- PSBT-like format for passing around partial transactions
- Validate M <= N and reasonable limits (e.g., N <= 15)

## Acceptance Criteria

- Can create 2-of-3 multisig address
- Can spend from multisig with threshold signatures
- Works on LAN testnet
