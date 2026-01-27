# pqcoin Roadmap

This document outlines the development roadmap for pqcoin, a post-quantum cryptocurrency.

## Vision

Build a production-ready, quantum-resistant cryptocurrency with:
- Battle-tested security through thorough auditing and testing
- Proven reliability through staged testnet deployments
- User-friendly wallet with modern features
- Performance optimizations for real-world usage

## Network Phases

pqcoin will progress through three distinct network phases, each with a **clean break** (no backwards compatibility):

| Phase | Network | Purpose | Compatibility |
|-------|---------|---------|---------------|
| 1 | LAN Testnet | Development testing on Tailscale | Disposable |
| 2 | WAN Testnet | Public testing on GCP | Disposable |
| 3 | Mainnet | Production network | Permanent |

---

## Priority 1: Production Hardening

Security and reliability improvements before any public deployment.

| Status | Issue | Tags |
|--------|-------|------|
| ✅ | [Prepare for Security Audit](roadmap/001-security-audit-prep.md) | security |
| ✅ | [Add Fuzzing Tests](roadmap/002-fuzzing-tests.md) | security, testing |
| ✅ | [Error Handling Audit](roadmap/003-error-handling-audit.md) | security |
| ⬜ | [Fix Fee Rate Bug - MIN_RELAY_FEE Units](roadmap/022-fix-fee-rate-bug---minrelayfee-units.md) | network, wallet |
| 🔄 | [Fix Header Sync Stalling Bug](roadmap/026-fix-header-sync-stalling-bug.md) | network |

---

## Priority 2: LAN Testnet (Tailscale)

Private testnet for development and initial testing.

**Nodes:**
- `samuel@desktop` - Seed node, miner
- `Samuels-MacBook-Pro` - Full node, wallet testing

| Status | Issue | Tags | Dependencies |
|--------|-------|------|--------------|
| ✅ | [Tailscale LAN Testnet Setup](roadmap/004-tailscale-testnet-setup.md) | infra, network | - |
| ✅ | [LAN Testnet Genesis Block](roadmap/005-lan-testnet-genesis.md) | network | 004 |
| ✅ | [LAN Testnet Validation](roadmap/006-lan-testnet-validation.md) | testing, network | 004, 005 |
| ✅ | [Fix getpeerinfo RPC](roadmap/025-fix-getpeerinfo-rpc---connect-to-network-state.md) | network, api | - |

---

## Priority 3: Wallet Improvements

Enhanced wallet functionality for better usability.

| Status | Issue | Tags |
|--------|-------|------|
| ✅ | [Multisig in CLI Wallet](roadmap/007-multisig-wallet.md) | wallet |
| ✅ | [HD Key Derivation](roadmap/008-hd-key-derivation.md) | wallet, security |
| ✅ | [Watch-Only Wallet Mode](roadmap/009-watch-only-mode.md) | wallet |
| ⬜ | [HD Wallet Address Switching](roadmap/023-hd-wallet-address-switching.md) | wallet |
| ⬜ | [Multisig Spend Command](roadmap/024-multisig-spend-command.md) | wallet |

---

## Priority 4: Performance

Optimizations for mining and block validation.

| Status | Issue | Tags |
|--------|-------|------|
| ⬜ | [GPU Mining Support](roadmap/010-gpu-mining.md) | mining, performance |
| ⬜ | [Block Validation Caching](roadmap/011-block-validation-cache.md) | performance |
| ⬜ | [Assume-Valid Optimization](roadmap/012-assume-valid.md) | performance |

---

## Priority 5: WAN Testnet (GCP)

Public testnet with multi-region infrastructure.

**Regions:** us-central1, europe-west1, asia-east1
**Block time:** 10 minutes (mainnet-like difficulty)

| Status | Issue | Tags | Dependencies |
|--------|-------|------|--------------|
| ⬜ | [GCP Infrastructure Setup](roadmap/013-gcp-infrastructure.md) | infra | - |
| ⬜ | [Deploy Seed Nodes](roadmap/014-seed-nodes.md) | infra, network | 001, 002, 003, 006, 013 |
| ⬜ | [WAN Testnet Genesis Block](roadmap/015-wan-testnet-genesis.md) | network | 006, 013 |
| ⬜ | [Testnet Faucet](roadmap/016-testnet-faucet.md) | infra, wallet | 014, 015 |
| ⬜ | [Testnet Documentation](roadmap/017-testnet-docs.md) | docs | 014, 015, 016 |

---

## Priority 6: Light Clients

Enable lightweight wallets without full blockchain download.

| Status | Issue | Tags |
|--------|-------|------|
| ⬜ | [SPV / Light Client Support](roadmap/018-spv-support.md) | network, wallet |

---

## Priority 7: Documentation

Comprehensive documentation for users and developers.

| Status | Issue | Tags | Dependencies |
|--------|-------|------|--------------|
| ⬜ | [JSON-RPC API Reference](roadmap/019-api-reference.md) | docs | - |
| ⬜ | [Network Protocol Specification](roadmap/020-protocol-spec.md) | docs, network | - |
| ⬜ | [Node Operation Tutorial](roadmap/021-node-tutorial.md) | docs | 017 |

---

## Progress

| Priority | Description | Issues | Completed |
|----------|-------------|--------|-----------|
| 1 | Production Hardening | 4 | 3 |
| 2 | LAN Testnet | 4 | 4 |
| 3 | Wallet Improvements | 5 | 3 |
| 4 | Performance | 3 | 0 |
| 5 | WAN Testnet | 5 | 0 |
| 6 | Light Clients | 1 | 0 |
| 7 | Documentation | 3 | 0 |
| **Total** | | **25** | **10** |

---

## Parallelization Guide

The following work streams can run **concurrently**:

```
┌─────────────────────────────────────────────────────────────────────┐
│                         PARALLEL WORK STREAMS                        │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Stream A: Production Hardening     Stream B: LAN Testnet           │
│  ┌─────┐ ┌─────┐ ┌─────┐           ┌─────┐                          │
│  │ 001 │ │ 002 │ │ 003 │           │ 004 │──→ 005 ──→ 006           │
│  └──✅─┘ └──✅─┘ └──✅─┘           └──✅─┘     ✅        ✅           │
│  ┌─────┐                                                            │
│  │ 022 │ (fee rate bug)                                             │
│  └─────┘                                                            │
│                                                                      │
│  Stream C: Wallet Features          Stream D: Performance           │
│  ┌─────┐ ┌─────┐ ┌─────┐           ┌─────┐ ┌─────┐ ┌─────┐          │
│  │ 007 │ │ 008 │ │ 009 │           │ 010 │ │ 011 │ │ 012 │          │
│  └──✅─┘ └──✅─┘ └──✅─┘           └─────┘ └─────┘ └─────┘          │
│  ┌─────┐ ┌─────┐                                                    │
│  │ 023 │ │ 024 │ (HD addr, multisig)                                │
│  └─────┘ └─────┘                                                    │
│                                                                      │
│  Stream E: GCP Setup (infra only)   Stream F: Documentation         │
│  ┌─────┐                            ┌─────┐ ┌─────┐                  │
│  │ 013 │                            │ 019 │ │ 020 │                  │
│  └─────┘                            └─────┘ └─────┘                  │
│                                                                      │
│  Stream G: Light Clients                                            │
│  ┌─────┐                                                            │
│  │ 018 │                                                            │
│  └─────┘                                                            │
│                                                                      │
├─────────────────────────────────────────────────────────────────────┤
│                      BLOCKED UNTIL GATES COMPLETE                    │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Gate: P1 + P2 + 013 complete                                       │
│  ┌─────┐                                                            │
│  │ 014 │──→ 016 ──→ 017 ──→ 021                                     │
│  └─────┘                                                            │
│  ┌─────┐    ↗                                                       │
│  │ 015 │───┘                                                        │
│  └─────┘                                                            │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

**Key gates:**
- 014 (Deploy Seed Nodes) requires: all of P1 (001-003) + 006 (LAN validation) + 013 (GCP infra)
- 015 (WAN Genesis) requires: 006 (LAN validation) + 013 (GCP infra)

This ensures we don't deploy publicly until hardening and LAN testing are complete.

---

## Contributing

1. Pick an issue from the roadmap above
2. Check its dependencies are completed
3. Read the full issue file in `roadmap/`
4. Open a PR with your implementation

### Status Legend

- ⬜ Planned
- 🔄 In Progress
- ✅ Completed
