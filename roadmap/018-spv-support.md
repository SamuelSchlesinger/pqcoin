---
title: "SPV / Light Client Support"
priority: 6
status: planned
tags: [network, wallet]
dependencies: []
---

# SPV / Light Client Support

Enable lightweight clients that don't download the full blockchain.

## Goals

- Mobile-friendly wallet without full node
- Fast sync for light clients
- Maintain security through SPV proofs

## Background

SPV (Simplified Payment Verification) clients:
- Download only block headers (~140 bytes each vs full blocks)
- Request Merkle proofs for their transactions
- Trust that longest PoW chain is valid

## Protocol Extensions

### New P2P Messages

```
getheaders - Request block headers
headers    - Block headers response
getproof   - Request Merkle proof for tx
proof      - Merkle proof response
```

### Bloom Filters (Privacy)

Optional bloom filter support so light clients don't reveal exact addresses:
```
filterload  - Set bloom filter
filterclear - Remove bloom filter
filteradd   - Add to filter
```

## Implementation Phases

1. **Header-only sync**: Download and verify header chain
2. **Merkle proofs**: Verify transaction inclusion
3. **Bloom filters**: Privacy-preserving queries (optional)

## Tasks

- [ ] Implement header chain storage
- [ ] Add getheaders/headers messages
- [ ] Implement Merkle proof generation
- [ ] Add getproof/proof messages
- [ ] Create light client mode for pqwallet
- [ ] Document SPV security model

## Acceptance Criteria

- Light client can sync headers in seconds
- Can verify own transactions with Merkle proofs
- Documented security tradeoffs
