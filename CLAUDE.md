# Claude Code Guidelines for pqcoin

## Project Overview

pqcoin is a post-quantum cryptocurrency using NIST-standardized cryptography:
- **ML-DSA-87** (FIPS 204) for signatures
- **SHA3-512** (FIPS 202) for hashing
- Bitcoin-inspired UTXO model with P2PKH and M-of-N multisig

---

## Roadmap System

**Always check the roadmap before starting work.**

### Quick Reference

```bash
# See all issues and their status
./scripts/roadmap-status.sh

# Search by tag
./scripts/roadmap-search.sh --tag security
./scripts/roadmap-search.sh --tag wallet --available

# Claim an issue to work on
./scripts/roadmap-claim.sh 001

# Mark issue complete
./scripts/roadmap-complete.sh 001

# Validate roadmap integrity
./scripts/roadmap-validate.sh
```

### Files

- `ROADMAP.md` - High-level overview with parallelization guide
- `roadmap/*.md` - Individual issue files with full details
- `scripts/roadmap-*.sh` - Automation scripts

### Workflow

1. Run `./scripts/roadmap-status.sh` to see available work
2. Pick an issue where all dependencies are complete (shown as ⬜ not 🚫)
3. Run `./scripts/roadmap-claim.sh NNN` to mark it in-progress
4. Read the full issue file in `roadmap/NNN-*.md` before implementing
5. Implement the changes
6. Run `./scripts/roadmap-complete.sh NNN` when done

**Important:** You MUST claim an issue before completing it. The scripts enforce this. If implementing multiple issues, claim each one before starting work:

```bash
# Claim before implementing
./scripts/roadmap-claim.sh 001
# ... implement issue 001 ...
./scripts/roadmap-complete.sh 001

# Or chain them for quick completion
./scripts/roadmap-claim.sh 001 && ./scripts/roadmap-complete.sh 001
```

### Dependency Rules

Issues with dependencies **must not start** until all dependencies are complete. The dependency graph enforces:
- No public deployment (WAN testnet) until production hardening + LAN testing complete
- Clean breaks between network phases (no backwards compatibility required)

### Issue File Format

```yaml
---
title: "Issue Title"
priority: 1-7
status: planned | in-progress | completed
tags: [security, network, wallet, mining, infra, docs, testing, performance]
dependencies: [001, 002]  # Must be lower-numbered issues
---

# Issue Title

Description and tasks...
```

---

## Network Phases

| Phase | Network | Genesis | Compatibility |
|-------|---------|---------|---------------|
| 1 | LAN Testnet | Disposable | Can reset anytime |
| 2 | WAN Testnet | Disposable | Clean break from LAN |
| 3 | Mainnet | Permanent | Clean break from WAN |

**Key insight:** Code can break compatibility between phases. Don't add backwards-compat hacks for testnet migrations.

---

## Architecture

```
src/
├── api/           # JSON-RPC and metrics endpoints
├── blockchain/    # Core chain logic, blocks, transactions, UTXO
├── mempool/       # Transaction pool and fee estimation
├── network/       # P2P networking and sync
├── storage/       # LMDB persistence layer
├── wallet/        # Key management and transaction building
├── bin/           # pqwallet CLI
├── config.rs      # Configuration handling
├── constants.rs   # Protocol constants
├── crypto.rs      # Cryptographic primitives wrapper
├── miner.rs       # Block mining
└── main.rs        # pqcoin node entry point
```

### Key Files

| File | Purpose |
|------|---------|
| `src/constants.rs` | Protocol constants (block time, rewards, etc.) |
| `src/blockchain/genesis.rs` | Genesis block creation |
| `src/blockchain/verification.rs` | Block/tx validation logic |
| `src/network/service/handlers.rs` | P2P message handling |
| `src/storage/lmdb.rs` | Database operations |

---

## Coding Conventions

### Rust Style

- Use `cargo fmt` before committing (pre-commit hook enforces this)
- Follow existing patterns in the codebase
- Prefer `thiserror` for error types
- Use `tracing` for logging, not `println!`

### Security-Critical Code

This is cryptocurrency software. Be paranoid:

- **No `unwrap()` on network/user input** - Always handle errors
- **Validate before processing** - Malformed data must not crash the node
- **Constant-time comparisons** for sensitive data where applicable
- **Document security assumptions** in comments

### Documentation Style

**Be helpful, not pedantic.** Only add comments that help future readers understand non-obvious design decisions.

**Good comments** (explain why, not what):
- `// Genesis is inserted in new() and never removed` - explains invariant
- `// SECURITY: Verify address binds to key before checking signature` - security rationale

**Avoid these** (stating the obvious):
- `// INVARIANT: SystemTime::now() is always after UNIX_EPOCH` - everyone knows this
- `// This parses the port from the config` - the code already says this

See `docs/error-handling-audit.md` for the codebase's `unwrap()`/`expect()` philosophy.

### Testing

- Unit tests go in `mod tests` within source files
- Integration tests go in `tests/integration/`
- Run `cargo test` before committing
- For network scenarios, use the test harness in `tests/integration/helpers.rs`

---

## Common Commands

```bash
# Build
cargo build --release

# Run tests
cargo test

# Run node with mining
./target/release/pqcoin --mine

# Run wallet
./target/release/pqwallet --help

# Format code
cargo fmt

# Pre-commit checks (manual)
./.githooks/pre-commit
```

---

## CI/CD

GitHub Actions runs on every push/PR:
- `cargo check` - Compilation
- `cargo fmt --check` - Formatting
- `cargo clippy` - Lints
- `cargo test` - Tests
- `cargo build --release` - Release build
- `./scripts/roadmap-validate.sh` - Roadmap integrity

See `.github/workflows/ci.yml` for details.

---

## LAN Testnet Setup (Priority 2)

Testnet nodes:
- `samuel@desktop` - Seed node, miner
- `Samuels-MacBook-Pro` - Full node, wallet testing

Connected via Tailscale. See `roadmap/004-tailscale-testnet-setup.md` for details.

---

## Process Improvement

### When to Reflect

At natural stopping points, consider whether the process could be improved:

**After completing a roadmap issue:**
- Did the issue description have enough detail?
- Were the dependencies correct?
- Should we add a new script to automate something repetitive?

**After hitting a blocker:**
- Is this a recurring problem that deserves a roadmap issue?
- Should we add validation to catch this earlier?
- Is there a missing dependency in the roadmap?

**After a successful test/deploy:**
- What manual steps could be automated?
- What documentation is missing?
- What would help the next person?

### What to Improve

Feel empowered to modify:

1. **Roadmap issues** - Add detail, fix dependencies, clarify acceptance criteria
2. **Scripts** - Add new `scripts/roadmap-*.sh` tools or improve existing ones
3. **CLAUDE.md** - Update this file with new learnings
4. **Pre-commit hooks** - Add validation for common mistakes
5. **CI workflow** - Add checks that catch issues earlier
6. **Architecture docs** - Keep `src/` documentation current

### Adding New Scripts

New scripts should:
- Live in `scripts/` with descriptive names
- Be executable (`chmod +x`)
- Include usage comments at the top
- Handle errors gracefully
- Be documented in this file

### Adding New Roadmap Issues

When scope expands:
1. Create `roadmap/NNN-descriptive-name.md` with next available number
2. Fill in YAML frontmatter (title, priority, status, tags, dependencies)
3. Add to `ROADMAP.md` in appropriate priority section
4. Run `./scripts/roadmap-validate.sh` to verify

---

## Maintenance

### Roadmap Tooling

The roadmap system is maintained via:
- `scripts/roadmap-start.sh` - Onboarding: shows guidelines + status
- `scripts/roadmap-status.sh` - Overview dashboard
- `scripts/roadmap-search.sh` - Search/filter issues
- `scripts/roadmap-claim.sh` - Claim an issue (validates deps)
- `scripts/roadmap-complete.sh` - Complete an issue
- `scripts/roadmap-new.sh` - Create new issue from template
- `scripts/roadmap-validate.sh` - Validate integrity
- `scripts/test-roadmap-scripts.sh` - Test suite for all scripts

**If you modify the YAML schema**, update:
1. The validation script
2. All roadmap-*.sh scripts that parse YAML
3. This documentation

### Pre-commit Hooks

The pre-commit hook (`.githooks/pre-commit`) runs:
1. `cargo fmt --check` - Code formatting
2. `cargo clippy` - Lints
3. Whitepaper PDF build (if pdflatex available)
4. Roadmap validation (if roadmap files changed)
5. Roadmap script tests (if scripts changed)

Enable with: `git config core.hooksPath .githooks`

### Testing Infrastructure Scripts

The roadmap scripts have their own test suite:

```bash
# Run all script tests
./scripts/test-roadmap-scripts.sh
```

Tests verify:
- Validation catches errors (invalid status, broken deps)
- Claim script enforces dependency rules
- Complete script updates status correctly
- Search/filter functions work
- New issue template is valid

**When modifying scripts**, always run the test suite. CI will also run these tests on every PR.

---

## Don't

- Don't add backwards compatibility for testnet migrations
- Don't commit with failing tests
- Don't use `unwrap()` on external input
- Don't start gated issues before dependencies complete
- Don't push to main/master without PR review
- Don't modify roadmap YAML schema without updating validation scripts
