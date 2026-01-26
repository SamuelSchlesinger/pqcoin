# Security Policy

## Threat Model

pqcoin is a post-quantum cryptocurrency designed to withstand both classical and quantum adversaries. This document describes the security assumptions, attack surfaces, and mitigations.

### Adversary Capabilities

1. **Network Attacker**: Can observe, delay, inject, or drop network traffic. Cannot break TLS or post-quantum cryptography.

2. **Malicious Peer**: A connected peer that sends malformed or malicious data. Can send invalid blocks, transactions, or protocol messages.

3. **Key Compromise**: An attacker who obtains a private key can spend funds associated with that key. Mitigated by standard key management practices.

4. **51% Attacker**: An attacker with majority hash power can reorg the chain. Standard PoW assumption; no special mitigation beyond PoW difficulty.

5. **Quantum Adversary**: A future attacker with a cryptographically-relevant quantum computer. Mitigated by ML-DSA-87 post-quantum signatures.

---

## Cryptographic Primitives

| Primitive | Algorithm | Standard | Security Level |
|-----------|-----------|----------|----------------|
| Signatures | ML-DSA-87 | FIPS 204 | NIST Level 5 (256-bit classical, 128-bit quantum) |
| Hashing | SHA3-512 | FIPS 202 | 256-bit classical, 128-bit quantum |

### ML-DSA-87 Properties

- **Public key size**: 2,592 bytes
- **Signature size**: 4,627 bytes
- **Verification**: Deterministic, constant-time (via pqcrypto-dilithium)

### SHA3-512 Properties

- **Output size**: 64 bytes (512 bits)
- **Usage**: Transaction IDs, block hashes, address derivation, PoW

---

## Attack Surfaces

### 1. P2P Network Layer

**Entry point**: `src/network/service/handlers.rs`

| Risk | Mitigation |
|------|------------|
| Message parsing crashes | All deserialization returns `Result`, no panics on malformed data |
| Memory exhaustion | Size limits enforced: `MAX_MESSAGE_SIZE` (10 MB), `MAX_BLOCK_TXS`, `MAX_TX_INPUTS`, `MAX_TX_OUTPUTS` |
| Connection flooding | Rate limiting via `RateLimiter`, subnet diversity via `SubnetLimiter` |
| Eclipse attacks | Peer rotation, outbound connection limits, address manager buckets |
| DoS amplification | GetAddr rate limiting (`GETADDR_RESPONSE_INTERVAL_SECS`) |

### 2. JSON-RPC API

**Entry point**: `src/api/rpc.rs`

| Risk | Mitigation |
|------|------------|
| Parameter injection | All parameters validated and typed via jsonrpsee |
| Information leakage | Generic error messages via `rpc_error()`, no stack traces |
| Resource exhaustion | Request size limits inherited from jsonrpsee defaults |

### 3. Metrics/Health Endpoints

**Entry points**: `src/api/health.rs`, `src/api/metrics.rs`

| Risk | Mitigation |
|------|------------|
| Information disclosure | Only exposes aggregate metrics, no private data |
| Denial of service | Read-only endpoints, minimal computation |

### 4. Wallet Key Management

**Entry point**: `src/wallet/mod.rs`

| Risk | Mitigation |
|------|------------|
| Key extraction | Encrypted at rest with AES-256-GCM |
| Weak passwords | Argon2id key derivation (memory-hard) |
| Memory exposure | `zeroize` crate for key material |

### 5. Consensus/Verification

**Entry point**: `src/blockchain/verification.rs`

| Risk | Mitigation |
|------|------------|
| Invalid signature acceptance | ML-DSA-87 verification via pqcrypto-dilithium |
| Double-spend | UTXO model with in-block double-spend detection |
| Merkle root forgery | Full merkle tree verification on every block |

---

## Network Entry Points

| Endpoint | Port | Protocol | Handler |
|----------|------|----------|---------|
| P2P | 8333 (default) | TCP | `src/network/service/handlers.rs` |
| JSON-RPC | 8332 (default) | HTTP | `src/api/rpc.rs` |
| Metrics | 9090 (default) | HTTP | `src/api/metrics.rs`, `src/api/health.rs` |

---

## Unsafe Code

### `src/storage/lmdb.rs:44-53`

The only `unsafe` block in the codebase. Required by the `heed` crate (LMDB wrapper) for environment initialization. See inline SAFETY comment for justification.

---

## Key Generation and Storage

### Key Generation

1. ML-DSA-87 keypair generated via `pqcrypto-dilithium`
2. Public key hashed with SHA3-512 to derive address
3. Private key never leaves memory unencrypted

### Key Storage (Wallet File)

1. Password processed through Argon2id (memory: 64 MB, iterations: 3, parallelism: 4)
2. Derived key used with AES-256-GCM
3. Random 12-byte nonce per encryption
4. File format: JSON with base64-encoded ciphertext

---

## Security Reporting

To report a security vulnerability, please email the maintainers directly. Do not open a public issue for security-sensitive matters.

---

## Security Checklist for Contributors

- [ ] No `unwrap()` or `expect()` on network/user input
- [ ] All deserialization validates length before allocation
- [ ] Error messages do not leak internal state
- [ ] New network handlers have size limits
- [ ] Cryptographic operations use constant-time implementations where possible
- [ ] Key material is zeroized after use
