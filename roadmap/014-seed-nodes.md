---
title: "Deploy Seed Nodes"
priority: 5
status: planned
tags: [infra, network]
dependencies: [001, 002, 003, 006, 013]
---

# Deploy Seed Nodes

Configure and deploy seed nodes on GCP infrastructure.

## Prerequisites

This issue is blocked until:
- **001, 002, 003** (Production Hardening) - Don't deploy code to public internet until security audit prep, fuzzing, and error handling are complete
- **006** (LAN Testnet Validation) - Don't go public until private testing passes
- **013** (GCP Infrastructure) - Need VMs to deploy to

## Goals

- Reliable entry points for new nodes joining the network
- Geographic distribution for resilience
- DNS seed support for easy discovery

## Seed Node Configuration

```toml
[network]
port = 8333
max_peers = 125
# Seed nodes know about each other
seeds = [
    "us-seed.testnet.pqcoin.org:8333",
    "eu-seed.testnet.pqcoin.org:8333",
    "asia-seed.testnet.pqcoin.org:8333"
]

[mining]
enabled = true  # Keep chain alive

[rpc]
enabled = true
port = 8332
bind = "0.0.0.0"
# Rate limiting for public RPC
rate_limit = 100  # requests per minute
```

## DNS Seeds

Set up DNS records:
- `seed.testnet.pqcoin.org` -> returns all seed node IPs
- Or individual: `us-seed.testnet.pqcoin.org`, etc.

## Tasks

- [ ] Deploy pqcoin binary to all seed VMs
- [ ] Configure systemd service for auto-restart
- [ ] Set up log rotation
- [ ] Configure DNS records
- [ ] Test node discovery from fresh client
- [ ] Set up uptime monitoring

## Acceptance Criteria

- All seed nodes running 24/7
- New nodes can bootstrap from seeds
- Automatic restart on crash
