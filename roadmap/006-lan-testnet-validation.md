---
title: "LAN Testnet Validation"
priority: 2
status: in-progress
tags: [testing, network]
dependencies: [004, 005]
---

# LAN Testnet Validation

Comprehensive testing on the Tailscale LAN testnet.

## Compatibility Note

**Feel free to break things.** This testnet exists to find bugs. Reset the chain as needed. All state will be discarded before the WAN testnet.

## Test Scenarios

### Block Propagation
- [ ] Mine block on desktop, verify it reaches MacBook
- [ ] Mine block on MacBook, verify it reaches desktop
- [ ] Measure propagation latency

### Chain Synchronization
- [ ] Stop one node, let the other mine blocks
- [ ] Restart stopped node, verify it syncs
- [ ] Test sync from scratch (empty data directory)

### Transaction Flow
- [ ] Create wallet on each node
- [ ] Send coins from miner to other wallet
- [ ] Verify UTXO updates on both nodes
- [ ] Test transaction propagation before mining

### Reorg Handling
- [ ] Disconnect nodes, mine on both
- [ ] Reconnect and verify reorg to longest chain
- [ ] Verify UTXO consistency after reorg

### Stress Testing
- [ ] Sustained mining over multiple hours
- [ ] Many transactions in mempool
- [ ] Node restart recovery

## Acceptance Criteria

- All test scenarios pass
- No crashes or data corruption
- Both nodes maintain consistent state
