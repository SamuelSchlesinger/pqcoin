# Error Handling Audit

This document records the audit of `unwrap()` and `expect()` usage in pqcoin production code.

## Summary

All production uses of `unwrap()` and `expect()` have been reviewed. They fall into these categories:

1. **SystemTime operations** - `duration_since(UNIX_EPOCH)` is infallible on any modern system
2. **Socket address parsing** - Format strings like `"0.0.0.0:{port}"` are always valid
3. **Data structure invariants** - Genesis block and chain tip always exist after initialization
4. **One-shot operations** - Receivers that are taken exactly once during startup

## Design Decisions

### Genesis Block and Chain Tip

Located in `src/blockchain/chain.rs`:

```rust
pub fn genesis(&self) -> &Block {
    self.blocks.get(&self.genesis_hash).unwrap()
}

pub fn tip(&self) -> &Block {
    self.blocks.get(&self.tip).unwrap()
}
```

These are intentionally infallible because:
- Genesis is inserted in `Blockchain::new()` and never removed
- Tip is initialized to genesis and only updated to valid blocks in `apply_block()`
- A missing genesis/tip indicates a serious bug, so panicking is appropriate

Returning `Option<&Block>` would push error handling to callers for a condition that should never occur.

### Network Service Startup

Located in `src/network/service/mod.rs`:

```rust
let mut peer_msg_rx = self.peer_msg_rx.take().expect("run called twice");
```

Uses the "take pattern" where a receiver is moved out of an Option. The expect message documents the invariant that `run()` is only called once.

## Guidelines for Future Development

### Use `unwrap()`/`expect()` when:
- The operation is mathematically infallible (e.g., parsing constant format strings)
- A failure indicates a bug, not a runtime error
- The invariant is obvious from context

### Use proper error handling when:
- Processing user input or network data
- Performing I/O operations
- Calling external services

## Verification

To check that no new unreviewed uses have been added:

```bash
# Find all unwrap/expect in production code
grep -rn "\.unwrap()\|\.expect(" src/ --include="*.rs" | grep -v "test"
```

Review any new occurrences against this document's guidelines.
