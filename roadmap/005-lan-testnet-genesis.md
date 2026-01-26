---
title: "LAN Testnet Genesis Block"
priority: 2
status: completed
tags: [network]
dependencies: [004]
---

# LAN Testnet Genesis Block

Create and deploy the genesis block for the Tailscale LAN testnet.

## Compatibility Note

**This genesis block is for LAN testing only.** It will be discarded when moving to the WAN testnet. No need to preserve any state or maintain compatibility.

## Genesis Parameters

| Parameter | Value | Rationale |
|-----------|-------|-----------|
| Timestamp | Current time | Fresh start |
| Difficulty | Low (testnet) | Fast initial blocks for testing |
| Initial reward | 50 coins | Match mainnet |

## Tasks

- [ ] Create testnet genesis block configuration
- [ ] Generate genesis block with known parameters
- [ ] Embed genesis in testnet build or config
- [ ] Deploy to both Tailscale nodes
- [ ] Verify both nodes accept the same genesis

## Implementation

Add a `--testnet` flag or `network = "testnet"` config option that uses the LAN testnet genesis.

## Acceptance Criteria

- Genesis block created with documented parameters
- Both nodes start with identical genesis
- Chain can grow from genesis
