---
title: "Prepare for Security Audit"
priority: 1
status: completed
tags: [security]
dependencies: []
---

# Prepare for Security Audit

Prepare the codebase for external security review.

## Goals

- Document all cryptographic operations and their security assumptions
- Create a threat model document
- Identify attack surfaces (P2P, RPC, wallet, consensus)
- Review all uses of `unsafe` code
- Ensure all cryptographic constants are properly sourced (NIST documents)

## Tasks

- [ ] Create SECURITY.md with threat model
- [ ] Audit all `unsafe` blocks and document justifications
- [ ] Review signature verification paths for timing attacks
- [ ] Document key generation and storage security
- [ ] List all network-facing entry points
- [ ] Review serialization/deserialization for malformed input handling

## Acceptance Criteria

- Comprehensive threat model document exists
- All unsafe code is justified and documented
- Security-critical code paths are identified and commented
