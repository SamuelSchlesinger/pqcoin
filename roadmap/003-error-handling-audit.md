---
title: "Error Handling Audit"
priority: 1
status: planned
tags: [security]
dependencies: []
---

# Error Handling Audit

Review and improve error handling throughout the codebase.

## Goals

- Ensure no panics in production code paths
- Proper error propagation without information leaks
- Graceful handling of malformed peer data

## Tasks

- [ ] Audit all `unwrap()` and `expect()` calls
- [ ] Replace panics with proper error returns in library code
- [ ] Ensure RPC errors don't leak internal state
- [ ] Review P2P message handling for DoS vectors
- [ ] Add error context for debugging without exposing internals
- [ ] Ensure database errors are handled gracefully

## Guidelines

- `unwrap()` acceptable only for: compile-time constants, test code, proven invariants
- All network input must be validated before processing
- Errors returned to peers should be generic (no stack traces)

## Acceptance Criteria

- No `unwrap()` on user/network input
- All error paths tested
- Node survives malformed input without crashing
