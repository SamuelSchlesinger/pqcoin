---
title: "GCP Infrastructure Setup"
priority: 5
status: planned
tags: [infra]
dependencies: []
---

# GCP Infrastructure Setup

Deploy public WAN testnet infrastructure on Google Cloud Platform.

## Compatibility Note

**Clean break from LAN testnet.** New genesis block, new chain. No state migrated from the Tailscale testnet. This testnet will also be discarded before mainnet launch.

## Architecture

Multi-region deployment for realistic network conditions:

| Region | Location | Purpose |
|--------|----------|---------|
| us-central1 | Iowa | Primary seed node |
| europe-west1 | Belgium | European seed node |
| asia-east1 | Taiwan | Asian seed node |

## Infrastructure Components

- **Seed nodes**: 3 VMs (one per region), always-on
- **Block explorer**: Optional web UI for testnet
- **Faucet service**: Distribute testnet coins

## VM Specifications

Seed nodes (minimal):
- e2-small (2 vCPU, 2GB RAM)
- 50GB SSD persistent disk
- Static external IP

## Tasks

- [ ] Set up GCP project for pqcoin
- [ ] Create Terraform/scripts for infrastructure
- [ ] Deploy VMs in each region
- [ ] Configure firewall rules (P2P port 8333)
- [ ] Set up monitoring and alerting
- [ ] Document deployment process

## Network Configuration

- 10-minute target block time (mainnet-like difficulty)
- Public P2P port: 8333
- Public RPC port: 8332 (seed nodes only, rate-limited)

## Acceptance Criteria

- VMs running in all 3 regions
- Nodes can connect to each other
- Infrastructure is reproducible via scripts
