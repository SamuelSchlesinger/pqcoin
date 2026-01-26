---
title: "Tailscale LAN Testnet Setup"
priority: 2
status: planned
tags: [infra, network]
dependencies: []
---

# Tailscale LAN Testnet Setup

Deploy a private testnet on Tailscale network for initial testing.

## Network Topology

| Node | Machine | Role |
|------|---------|------|
| Node 1 | samuel@desktop | Seed node, miner |
| Node 2 | Samuels-MacBook-Pro | Full node, wallet testing |

## Compatibility Note

**This testnet has no backwards compatibility requirements.** The chain can be reset at any time. This is for development testing only. A clean break will occur when moving to the WAN testnet.

## Tasks

- [ ] Build release binaries on both machines
- [ ] Configure Tailscale connectivity between nodes
- [ ] Set up node configuration files
- [ ] Configure nodes to discover each other via Tailscale IPs
- [ ] Start nodes and verify P2P connectivity

## Configuration

Each node needs a `pqcoin.toml`:

```toml
[network]
port = 8333
# Add the other node's Tailscale IP
seeds = ["100.x.x.x:8333"]

[mining]
enabled = true  # Enable on at least one node

[rpc]
enabled = true
port = 8332
```

## Acceptance Criteria

- Both nodes running and connected via Tailscale
- Nodes can discover and connect to each other
- P2P message exchange working
