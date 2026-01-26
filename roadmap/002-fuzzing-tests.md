---
title: "Add Fuzzing Tests"
priority: 1
status: planned
tags: [security, testing]
dependencies: []
---

# Add Fuzzing Tests

Implement fuzz testing for security-critical parsing and validation code.

## Goals

- Find edge cases in deserialization code
- Test consensus-critical validation logic
- Discover potential DoS vectors

## Targets

- [ ] Block deserialization
- [ ] Transaction deserialization
- [ ] P2P message parsing
- [ ] Script/output type parsing
- [ ] Signature verification with malformed inputs
- [ ] Merkle tree construction

## Implementation

Use `cargo-fuzz` with libFuzzer:

```bash
cargo install cargo-fuzz
cargo fuzz init
cargo fuzz add block_deserialize
cargo fuzz run block_deserialize
```

## Acceptance Criteria

- Fuzz targets exist for all deserialization code
- CI runs fuzz tests (time-limited) on each PR
- Any crashes found are fixed
