---
title: "Block Validation Caching"
priority: 4
status: planned
tags: [performance]
dependencies: []
---

# Block Validation Caching

Cache expensive validation results to speed up sync and reorgs.

## Goals

- Avoid re-verifying signatures for known-valid blocks
- Speed up initial block download (IBD)
- Faster reorg processing

## What to Cache

1. **Signature validity** - ML-DSA verification is expensive (~1ms per sig)
2. **Script/output validation** - Less expensive but still worth caching
3. **Merkle roots** - Avoid recomputation

## Design

```rust
struct ValidationCache {
    // Block hash -> validation status
    block_validity: LruCache<Hash, BlockValidity>,

    // Transaction hash -> signature valid
    tx_signatures: LruCache<Hash, bool>,
}

enum BlockValidity {
    Valid,
    InvalidSignature(TxIndex),
    InvalidStructure(String),
}
```

## Cache Invalidation

- Cache entries are immutable (a tx hash always has same validity)
- Use LRU eviction for memory management
- Persist cache to disk for faster restarts (optional)

## Tasks

- [ ] Implement validation cache structure
- [ ] Integrate with block validation pipeline
- [ ] Add cache hit/miss metrics
- [ ] Benchmark IBD with and without cache

## Acceptance Criteria

- Signature verification skipped for cached transactions
- Measurable IBD speedup (target: 2x+)
- Memory usage bounded by LRU limits
