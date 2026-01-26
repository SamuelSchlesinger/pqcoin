---
title: "Watch-Only Wallet Mode"
priority: 3
status: planned
tags: [wallet]
dependencies: []
---

# Watch-Only Wallet Mode

Support wallets that can view balances but not spend.

## Goals

- Monitor addresses without exposing private keys
- Cold storage workflow support
- Create unsigned transactions for offline signing

## Use Cases

1. **Cold storage**: Watch hot wallet from online machine, sign on air-gapped machine
2. **Monitoring**: Track addresses without spending capability
3. **Shared visibility**: Multiple parties can monitor same addresses

## Commands

```bash
# Create watch-only wallet from public key
pqwallet watch --pubkey <pubkey>

# Or from address
pqwallet watch --address <address>

# Check balance (works same as normal)
pqwallet balance

# Create unsigned transaction
pqwallet send --unsigned <address> <amount> --output unsigned.tx

# On offline machine with full wallet:
pqwallet sign --tx unsigned.tx --output signed.tx

# Back on online machine:
pqwallet broadcast --tx signed.tx
```

## Implementation

- Store only public keys in watch-only wallet file
- Refuse signing operations with clear error message
- Support importing unsigned tx from file

## Acceptance Criteria

- Can create watch-only wallet
- Balance checking works
- Can create and broadcast offline-signed transactions
