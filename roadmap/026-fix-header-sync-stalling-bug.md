---
title: "Fix Header Sync Stalling Bug"
priority: 1
status: in-progress
tags: [network]
dependencies: []
---

# Fix Header Sync Stalling Bug

## Overview

Initial block download (IBD) stalls after receiving a partial batch of headers. The sync logic assumes header download is complete when fewer than 2000 headers are received, but this can happen for various reasons (slow disk, network batching, etc.) even when more headers are available.

**Symptom:** Node syncs to exactly 432 blocks (or similar partial count) then stops, while peers report heights of 7000+.

**Root cause:** In `src/network/sync.rs:359-367`:

```rust
// If we got a full batch (2000 headers) AND accepted all of them, request more
if headers_len >= MAX_HEADERS_COUNT && accepted_count == headers_len {
    self.set_state(SyncState::DownloadingHeaders);
    responses.push(self.create_get_headers_message().await);
} else {
    self.set_state(SyncState::DownloadingBlocks);  // BUG
}
```

When a peer sends fewer than 2000 headers, we immediately transition to `DownloadingBlocks` instead of requesting more headers. We should only transition to block download when:
1. We receive an empty headers response, OR
2. Our tip height matches or exceeds the peer's reported height

## Tasks

- [ ] Track best known peer height in SyncManager
- [ ] Modify `process_headers()` to request more headers if tip < best_known_height
- [ ] Only transition to `DownloadingBlocks` when headers are truly complete
- [ ] Add logging to track header sync progress (current height vs target)
- [ ] Add test for partial header batch scenario
- [ ] Verify fix on LAN testnet with fresh sync

## Acceptance Criteria

- [ ] Fresh node syncs fully to peer height (7000+ blocks) without stalling
- [ ] Partial header batches trigger more header requests
- [ ] Sync only transitions to block download when caught up
- [ ] No regression in full-batch header scenarios
