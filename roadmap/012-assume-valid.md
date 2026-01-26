---
title: "Assume-Valid Optimization"
priority: 4
status: planned
tags: [performance]
dependencies: []
---

# Assume-Valid Optimization

Skip signature verification for blocks under a known-valid checkpoint.

## Concept

Bitcoin Core's `-assumevalid` flag specifies a block hash that is known to be valid. During IBD, signature verification is skipped for all blocks up to this point, dramatically speeding up sync.

This is safe because:
- The checkpoint is distributed with the software
- Blocks still have PoW verified
- Only signatures are skipped, not structural validation

## Implementation

```rust
// In config or hardcoded
const ASSUME_VALID: Option<Hash> = Some(hash!("00000000..."));

fn validate_block(block: &Block, height: u64) -> Result<()> {
    // Always verify PoW
    verify_pow(block)?;

    // Skip signatures if under assume-valid
    if should_skip_signatures(height) {
        verify_structure_only(block)?;
    } else {
        verify_full(block)?;
    }

    Ok(())
}
```

## Tasks

- [ ] Add `--assumevalid` CLI flag
- [ ] Add config file option
- [ ] Skip signature verification when applicable
- [ ] Document the security model
- [ ] Update assume-valid hash in releases

## Configuration

```toml
[sync]
# Skip signature verification up to this block
assume_valid = "00000000..."
```

## Acceptance Criteria

- IBD significantly faster with assume-valid
- Full verification still works without flag
- Clear documentation of trust assumptions
