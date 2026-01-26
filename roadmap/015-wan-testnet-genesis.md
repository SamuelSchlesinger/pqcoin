---
title: "WAN Testnet Genesis Block"
priority: 5
status: planned
tags: [network]
dependencies: [006, 013]
---

# WAN Testnet Genesis Block

Create the genesis block for the public WAN testnet.

## Prerequisites

This issue is blocked until:
- **006** (LAN Testnet Validation) - Don't launch public chain until private testing validates the implementation
- **013** (GCP Infrastructure) - Need infrastructure to deploy genesis to

## Compatibility Note

**Clean break from LAN testnet.** This is a completely new chain with a new genesis block. No backwards compatibility with the Tailscale testnet. This chain will also be reset before mainnet.

## Genesis Parameters

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Timestamp | Launch date | Mark public testnet launch |
| Difficulty | Mainnet-like | 10-minute blocks for realistic testing |
| Initial reward | 50 coins | Match mainnet |
| Message | "pqcoin WAN testnet launch" | Identify the network |

## Difficulty Consideration

Using mainnet-like difficulty (10-minute blocks):
- Pros: Realistic testing, proper difficulty adjustment testing
- Cons: Slower initial testing, need mining power

With 3 seed nodes mining, initial difficulty should be calibrated to achieve ~10 minute blocks.

## Tasks

- [ ] Generate new genesis block
- [ ] Calculate appropriate initial difficulty
- [ ] Embed in testnet build
- [ ] Deploy to all seed nodes
- [ ] Verify all nodes on same chain
- [ ] Document genesis parameters publicly

## Implementation

Add network selection:
```bash
pqcoin --network testnet  # Uses WAN testnet genesis
pqcoin --network lantest  # Uses LAN testnet genesis (dev only)
pqcoin --network mainnet  # Future mainnet genesis
```

## Acceptance Criteria

- Genesis block created and documented
- All seed nodes running same genesis
- Network achieves ~10 minute block times
