---
title: "Fix getpeerinfo RPC - Connect to Network State"
priority: 2
status: completed
tags: [network, api]
dependencies: []
---

# Fix getpeerinfo RPC - Connect to Network State

## Overview

The `getpeerinfo` RPC endpoint is hardcoded to return an empty vector, making it impossible to query connected peers. This is an architectural issue where the RPC/API layer is disconnected from the NetworkService's peer state.

**Current behavior** (`src/api/rpc.rs:248-250`):
```rust
async fn get_peer_info(&self) -> RpcResult<Vec<PeerInfo>> {
    // Return empty for now - peer info requires network service access
    Ok(vec![])
}
```

**Impact:**
- `getpeerinfo` always returns `[]` even with active connections
- `connected_peers` metric always shows 0
- No way to debug P2P connectivity via RPC

## Root Cause Analysis

1. **NetworkService owns peer state** - `NetworkState.peers` HashMap is the ground truth
2. **NetworkService moved into spawn** - After `tokio::spawn(service.run())`, it's inaccessible
3. **ApiState has no access** - Only stores `peer_count` atomic (never updated)
4. **One-way event flow** - Network events go to event handler, not ApiState

## Architecture Issue

```
Current:
NetworkService (owns peers) → tokio::spawn() → inaccessible
ApiState (no peer access) → RPC handlers → return empty

Needed:
NetworkService → SharedPeerState (Arc<RwLock<>>) ← ApiState
                                                  ↓
                                           RPC returns real data
```

## Tasks

- [ ] Extract peer state into shared `Arc<RwLock<NetworkState>>` or add query channel
- [ ] Pass shared state reference to ApiState
- [ ] Implement actual `get_peer_info()` that queries shared state
- [ ] Add peer count updates on connect/disconnect events
- [ ] Update metrics endpoint to show real peer count
- [ ] Add integration test verifying getpeerinfo returns connected peers

## Implementation Options

### Option A: Shared State (simpler)
- Extract `NetworkState` into `Arc<RwLock<NetworkState>>`
- Pass to both NetworkService and ApiState
- RPC handlers lock and read directly

### Option B: Query Channel (more isolated)
- Add `mpsc` channel for queries to NetworkService
- ApiState sends query, awaits response
- Better isolation but more complex

## Acceptance Criteria

- [ ] `getpeerinfo` returns actual connected peers with correct info
- [ ] `connected_peers` metric reflects real peer count
- [ ] Peer info includes: address, height, connection time, inbound/outbound
- [ ] No performance regression from state sharing
