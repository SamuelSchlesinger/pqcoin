---
title: "HD Wallet Address Switching"
priority: 3
status: planned
tags: [wallet]
dependencies: []
---

# HD Wallet Address Switching

## Overview

HD wallets can derive multiple addresses, but there's no way to switch the "current" address back to a previous one. Additionally, the wallet only checks the current address for UTXOs when sending.

**Current limitations:**
1. `pqwallet address new` derives a new address and sets it as current
2. No `pqwallet address set <index>` to switch to a previous address
3. `pqwallet send` only queries UTXOs for the current address
4. `pqwallet balance` only shows balance for the current address

This causes problems when:
- Mining rewards go to address 0, but wallet is now on address 2
- User wants to consolidate funds from multiple addresses
- User accidentally derives a new address and loses access to previous funds

## Tasks

- [ ] Add `pqwallet address set <index>` command to switch current address
- [ ] Update `send` to check UTXOs across all derived addresses (or optionally just current)
- [ ] Update `balance` to show total across all derived addresses
- [ ] Add `--address-index` flag to `send` for explicit address selection
- [ ] Consider: should `address new` prompt before changing current address?

## Acceptance Criteria

- [ ] User can switch between derived addresses
- [ ] Balance shows total across all addresses by default
- [ ] Send can spend from any derived address
- [ ] Clear UX for managing multiple addresses
