---
title: "Fix Fee Rate Bug - MIN_RELAY_FEE Units"
priority: 1
status: planned
tags: [network, wallet]
dependencies: []
---

# Fix Fee Rate Bug - MIN_RELAY_FEE Units

## Overview

The mempool fee rate check has a critical bug where `MIN_RELAY_FEE = 1000` is used as **quanta per byte** instead of a total minimum fee or quanta per KB.

**Current behavior** (`src/mempool/mod.rs:119-120`):
```rust
let fee_rate = if tx_size > 0 { fee / tx_size } else { 0 };
if fee_rate < MIN_RELAY_FEE {  // MIN_RELAY_FEE = 1000
```

With ML-DSA-87 signatures being ~4600 bytes, a single-input transaction requires:
- ~5000 bytes total x 1000 quanta/byte = **50 PQC minimum fee!**

This is prohibitively expensive. Bitcoin uses ~1 sat/vbyte minimum.

## Root Cause

The constant `MIN_RELAY_FEE = 1000` was intended as a total minimum fee in quanta, but the comparison treats it as a per-byte rate.

## Tasks

- [ ] Decide on fee rate semantics: quanta per byte vs quanta per KB vs flat minimum
- [ ] Update `src/mempool/mod.rs` fee rate calculation
- [ ] Update `src/constants.rs` comments to clarify units
- [ ] Ensure wallet fee estimation matches mempool expectations
- [ ] Add tests for fee rate edge cases

## Acceptance Criteria

- [ ] Single-input transaction with reasonable fee (< 1 PQC) is accepted
- [ ] Multi-input transactions are economically viable
- [ ] Fee rate semantics are clearly documented
- [ ] Existing tests pass with new fee logic
