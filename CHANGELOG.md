# Changelog

All notable changes to pqcoin are documented in this file.

## [Unreleased]

### Performance Improvements

#### Transaction Index for Fast Reorg Lookups
- Added `tx_index: HashMap<Hash, Hash>` to `Blockchain` struct mapping txid to block hash
- `find_block_containing_tx()` now performs O(1) lookups instead of O(n*m) scans
- Significantly improves chain reorganization performance for deep reorgs
- Index is automatically maintained during `apply_block()` and `unapply_block()`

#### Fee-Prioritized Transaction Selection
- Miner now uses `get_block_txs_with_fees()` instead of `get_block_txs()`
- Transactions are sorted by fee rate (fee / serialized size) in descending order
- Maximizes miner revenue by including highest-paying transactions first

#### Network Message Memory Optimization
- Reduced peak memory usage for large messages by 50%
- Single allocation for header + payload instead of separate allocations
- For a 10MB message: now uses 10MB peak instead of 20MB

### Security Enhancements

#### Explicit String Size Limit
- Added `MAX_STRING_SIZE` constant (1 MB) in network message parsing
- Provides defense-in-depth against oversized string allocations
- Complements existing message size limits for comprehensive protection

### Documentation

#### Mempool Transaction Dependencies
- Documented that chained unconfirmed transactions are not supported
- Added comprehensive module-level documentation explaining:
  - Why outputs from mempool transactions cannot be spent
  - Implications for wallets and batch operations
  - Rationale for this design simplification
- Added test case `test_chained_transactions_not_supported()` demonstrating expected behavior

## Design Decisions

### Chained Transactions Not Supported

The mempool intentionally does not support spending outputs created by other
unconfirmed transactions (CPFP). This is a deliberate simplification:

**Rationale:**
- Reduces mempool complexity and validation overhead
- Aligns with pqcoin's "simplified post-quantum cryptocurrency" design
- Most real-world usage (sequential transactions) is unaffected

**Implications:**
- Wallets must wait for confirmation before spending new outputs
- Fee-bumping via Child-Pays-For-Parent is not available
- Batch operations must be submitted sequentially

### Serialization: Version in txid

The transaction version field IS included in the txid hash. This is intentional:
- Different versions represent different transaction formats
- Matching Bitcoin's original approach
- Transaction malleability is prevented through separate `signing_data()` method

### Timestamp Validation

Blocks allow timestamps up to 2 hours in the future (`MAX_FUTURE_BLOCK_TIME`):
- Standard approach matching Bitcoin
- Combined with 4x difficulty adjustment clamp
- Provides adequate protection against timestamp manipulation
