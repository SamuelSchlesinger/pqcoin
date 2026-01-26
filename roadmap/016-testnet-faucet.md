---
title: "Testnet Faucet"
priority: 5
status: planned
tags: [infra, wallet]
dependencies: [014, 015]
---

# Testnet Faucet

Provide a way for users to obtain testnet coins.

## Goals

- Easy onboarding for testnet users
- Rate-limited to prevent abuse
- Simple web or CLI interface

## Design Options

### Option A: Web Faucet
- Simple web page with address input
- CAPTCHA to prevent bots
- Rate limit per IP and address

### Option B: CLI Faucet
- `pqwallet faucet request` command
- Talks to faucet API
- Simpler to implement

### Option C: Discord/Telegram Bot
- Request coins via chat command
- Rate limit per user account
- Community engagement

Recommend starting with **Option A** (web faucet) for accessibility.

## Implementation

Simple web service:
```
POST /api/faucet
{
    "address": "pq1..."
}

Response:
{
    "txid": "abc123...",
    "amount": 10.0
}
```

## Rate Limits

- 10 coins per request
- 1 request per address per day
- 5 requests per IP per day

## Tasks

- [ ] Implement faucet backend
- [ ] Create simple web frontend
- [ ] Deploy alongside seed nodes
- [ ] Fund faucet wallet from mining
- [ ] Add rate limiting
- [ ] Monitor faucet balance

## Acceptance Criteria

- Users can request testnet coins
- Rate limiting prevents abuse
- Faucet stays funded
