# Configuration Templates

This directory contains example configuration files for running pqcoin nodes.

## LAN Testnet Setup

For a basic two-node LAN testnet using Tailscale:

### Prerequisites

1. Install Tailscale on both machines
2. Ensure both machines are connected to the same Tailscale network
3. Note the Tailscale IP addresses (usually `100.x.x.x`)

### Node 1: Desktop (Seed Node + Miner)

1. Copy configuration:
   ```bash
   mkdir -p ~/.pqcoin
   cp config/node1-desktop.toml.example ~/.pqcoin/config.toml
   ```

2. Edit `~/.pqcoin/config.toml`:
   - Set `advertise_addr` to your desktop's Tailscale IP
   - Create a wallet and set `miner_address`:
     ```bash
     pqwallet create
     pqwallet address
     # Copy the address to config.toml
     ```

3. Start the node:
   ```bash
   ./target/release/pqcoin --mine
   ```

### Node 2: MacBook (Full Node)

1. Copy configuration:
   ```bash
   mkdir -p ~/.pqcoin
   cp config/node2-macbook.toml.example ~/.pqcoin/config.toml
   ```

2. Edit `~/.pqcoin/config.toml`:
   - Set `advertise_addr` to your MacBook's Tailscale IP
   - Set `bootstrap_peers` to the desktop's Tailscale IP

3. Start the node:
   ```bash
   ./target/release/pqcoin
   ```

### Verification

After both nodes are running:

```bash
# On either node, check peer connections
curl -X POST http://localhost:8332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getpeerinfo","params":[],"id":1}'

# Check blockchain sync
curl -X POST http://localhost:8332 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockchaininfo","params":[],"id":1}'
```

## Files

- `node1-desktop.toml.example` - Seed node configuration (mining enabled)
- `node2-macbook.toml.example` - Full node configuration (connects to seed)
