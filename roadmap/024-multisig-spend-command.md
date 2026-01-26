---
title: "Multisig Spend Command"
priority: 3
status: planned
tags: [wallet]
dependencies: []
---

# Multisig Spend Command

## Overview

The current multisig workflow is incomplete - there's no way to create an unsigned transaction from a multisig address. The existing commands are:

- `pqwallet multisig create` - Creates multisig address from pubkeys
- `pqwallet multisig sign` - Signs an existing unsigned transaction
- `pqwallet multisig combine` - Combines partial signatures

**Missing:** A way to create the initial unsigned transaction for the multisig to spend.

**Current workaround attempt (fails):**
1. Create watch-only wallet with `--address <multisig_id>`
2. Use `send --unsigned` from watch-only wallet
3. **Problem:** Watch-only with just an address "cannot create transactions"

## Proposed Solution

Add `pqwallet multisig spend` command:

```bash
pqwallet multisig spend \
  --multisig <MULTISIG_ID> \
  --to <RECIPIENT> \
  --amount <AMOUNT> \
  --fee <FEE> \
  --output /tmp/unsigned-tx.json
```

This would:
1. Query UTXOs for the multisig address
2. Build an unsigned transaction
3. Include multisig metadata (M-of-N, pubkeys) for signers
4. Output to file for signing workflow

## Tasks

- [ ] Add `multisig spend` subcommand to pqwallet
- [ ] Store multisig definitions so they can be referenced by ID
- [ ] Include multisig script in unsigned transaction output
- [ ] Update `multisig sign` to extract script from transaction
- [ ] Add integration test for full multisig workflow
- [ ] Document multisig workflow in help text

## Acceptance Criteria

- [ ] Complete multisig spend workflow works end-to-end
- [ ] 2-of-2 multisig can be funded, spent, and confirmed
- [ ] 2-of-3 multisig works with any 2 signers
- [ ] Clear error messages for missing signatures
