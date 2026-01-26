---
title: "JSON-RPC API Reference"
priority: 7
status: planned
tags: [docs]
dependencies: []
---

# JSON-RPC API Reference

Comprehensive documentation for the RPC API.

## Goals

- Complete reference for all RPC methods
- Request/response examples
- Error codes and handling

## Documentation Structure

For each method:
- Description
- Parameters (with types)
- Return value (with types)
- Example request
- Example response
- Error cases

## Current Methods to Document

### Blockchain
- [ ] `getblockchaininfo` - Chain state and sync status
- [ ] `getblock` - Get block by hash
- [ ] `getblockhash` - Get hash by height
- [ ] `getblockheader` - Get header only
- [ ] `getdifficulty` - Current difficulty

### Transactions
- [ ] `sendrawtransaction` - Broadcast transaction
- [ ] `getrawtransaction` - Get transaction by hash
- [ ] `gettxout` - Get specific UTXO

### Wallet
- [ ] `getbalance` - Address or wallet balance
- [ ] `getutxos` - List unspent outputs
- [ ] `getnewaddress` - Generate address

### Network
- [ ] `getpeerinfo` - Connected peers
- [ ] `getnetworkinfo` - Network status
- [ ] `addpeer` - Manually connect to peer

### Mining
- [ ] `getmininginfo` - Mining status
- [ ] `getblocktemplate` - Template for miners

## Format

```markdown
## getblock

Get a block by its hash.

**Parameters:**
| Name | Type | Required | Description |
|------|------|----------|-------------|
| hash | string | Yes | Block hash (hex) |
| verbosity | int | No | 0=hex, 1=json, 2=json+tx |

**Returns:**
Block object or hex string depending on verbosity.

**Example:**
...
```

## Acceptance Criteria

- All RPC methods documented
- Examples are tested and accurate
- Published in accessible location
