//! Block synchronization logic.
//!
//! This module handles synchronizing the blockchain with peers:
//! - Initial Block Download (IBD)
//! - Header-first synchronization
//! - Block downloading and validation
//! - Orphan block handling

use crate::blockchain::{Block, BlockHeader, Blockchain, BlockchainError};
use crate::constants::{MAX_BLOCKS_IN_FLIGHT, MAX_HEADERS_COUNT, MAX_ORPHAN_BLOCKS, MAX_PENDING_HEADERS};
use crate::crypto::Hash;
use crate::network::message::{InvItem, InvType, Message};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Timeout for block downloads (30 seconds).
const BLOCK_DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30);

/// State of synchronization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncState {
    /// Initial state, determining what to sync.
    Idle,
    /// Downloading headers to find the best chain.
    DownloadingHeaders,
    /// Downloading blocks to catch up.
    DownloadingBlocks,
    /// Synchronized with the network.
    Synced,
}

/// Tracks which blocks are being downloaded from which peer.
#[derive(Debug, Clone)]
struct BlockDownload {
    /// ID of the peer we're downloading from.
    peer_id: u64,
    /// When the request was sent (for timeout tracking).
    requested_at: Instant,
}

/// Tracks sync state for a specific peer.
#[derive(Debug, Clone)]
struct PeerSyncState {
    /// Headers this peer has advertised.
    known_headers: HashSet<Hash>,
    /// Chain tip this peer reported.
    tip_hash: Hash,
    /// Height of the tip this peer reported.
    tip_height: u64,
    /// Are we actively syncing from this peer?
    syncing_from: bool,
}

impl PeerSyncState {
    fn new() -> Self {
        Self {
            known_headers: HashSet::new(),
            tip_hash: Hash::from_bytes([0u8; 64]),
            tip_height: 0,
            syncing_from: false,
        }
    }
}

/// Manages block synchronization with the network.
pub struct SyncManager {
    /// Reference to the blockchain.
    blockchain: Arc<RwLock<Blockchain>>,
    /// Current sync state.
    state: SyncState,
    /// Headers we've received but haven't processed blocks for yet.
    pending_headers: VecDeque<BlockHeader>,
    /// Set of block hashes we're currently downloading.
    blocks_in_flight: HashMap<Hash, BlockDownload>,
    /// Headers we've downloaded, indexed by hash (for re-queuing on disconnect).
    downloaded_headers: HashMap<Hash, BlockHeader>,
    /// Orphan blocks (blocks whose parent we don't have yet), with originating peer_id.
    orphan_blocks: HashMap<Hash, (Block, u64)>,
    /// LRU tracking for orphan blocks - oldest hashes at front.
    orphan_order: VecDeque<Hash>,
    /// Per-peer sync state tracking.
    peer_states: HashMap<u64, PeerSyncState>,
    /// Which peer's chain we're currently syncing from.
    active_sync_peer: Option<u64>,
    /// Peers that have been synced with (sent us headers).
    synced_peers: HashSet<u64>,
    /// Best known height across all peers.
    best_known_height: u64,
    /// Peer with the best chain.
    best_peer: Option<u64>,
}

impl SyncManager {
    /// Create a new sync manager.
    pub fn new(blockchain: Arc<RwLock<Blockchain>>) -> Self {
        Self {
            blockchain,
            state: SyncState::Idle,
            pending_headers: VecDeque::new(),
            blocks_in_flight: HashMap::new(),
            downloaded_headers: HashMap::new(),
            orphan_blocks: HashMap::new(),
            orphan_order: VecDeque::new(),
            peer_states: HashMap::new(),
            active_sync_peer: None,
            synced_peers: HashSet::new(),
            best_known_height: 0,
            best_peer: None,
        }
    }

    /// Add an orphan block with LRU eviction if at capacity.
    fn add_orphan(&mut self, hash: Hash, block: Block, peer_id: u64) {
        // Evict oldest orphans if at capacity
        while self.orphan_blocks.len() >= MAX_ORPHAN_BLOCKS {
            if let Some(old_hash) = self.orphan_order.pop_front() {
                self.orphan_blocks.remove(&old_hash);
                tracing::debug!(hash = %old_hash, "evicted orphan block (LRU)");
            } else {
                break;
            }
        }

        // Add new orphan
        if self.orphan_blocks.insert(hash, (block, peer_id)).is_none() {
            // Only add to order if it's a new entry
            self.orphan_order.push_back(hash);
        }
    }

    /// Remove an orphan block.
    fn remove_orphan(&mut self, hash: &Hash) -> Option<(Block, u64)> {
        if let Some(data) = self.orphan_blocks.remove(hash) {
            // Remove from order tracking - this is O(n) but orphan set is small
            self.orphan_order.retain(|h| h != hash);
            Some(data)
        } else {
            None
        }
    }

    /// Get the current sync state.
    pub fn state(&self) -> SyncState {
        self.state
    }

    /// Get the number of blocks in flight.
    pub fn blocks_in_flight(&self) -> usize {
        self.blocks_in_flight.len()
    }

    /// Check if we're syncing.
    pub fn is_syncing(&self) -> bool {
        matches!(
            self.state,
            SyncState::DownloadingHeaders | SyncState::DownloadingBlocks
        )
    }

    /// Update the best known height from a peer.
    pub fn update_peer_height(&mut self, peer_id: u64, height: u64) {
        if height > self.best_known_height {
            self.best_known_height = height;
            self.best_peer = Some(peer_id);
        }
    }

    /// Build a block locator for requesting headers.
    ///
    /// The locator contains hashes at exponentially increasing distances
    /// back from the tip, allowing peers to find the common ancestor quickly.
    pub async fn build_locator(&self) -> Vec<Hash> {
        let blockchain = self.blockchain.read().await;
        let height = blockchain.height();
        let mut locator = Vec::new();

        // Include recent blocks
        let mut step = 1u64;
        let mut index = height;

        loop {
            if let Some(hash) = blockchain.hash_at_height(index) {
                locator.push(hash);
            }

            if index == 0 {
                break;
            }

            // Exponentially increase step after first 10 blocks
            if locator.len() >= 10 {
                step *= 2;
            }

            index = index.saturating_sub(step);
        }

        // Always include genesis
        if let Some(genesis_hash) = blockchain.hash_at_height(0) {
            if locator.last() != Some(&genesis_hash) {
                locator.push(genesis_hash);
            }
        }

        locator
    }

    /// Create a GetHeaders message to request headers.
    pub async fn create_get_headers_message(&self) -> Message {
        let locator = self.build_locator().await;
        Message::GetHeaders {
            locator,
            stop: Hash::from_bytes([0u8; 64]), // No limit
        }
    }

    /// Process received headers.
    pub async fn process_headers(
        &mut self,
        peer_id: u64,
        headers: Vec<BlockHeader>,
    ) -> Result<Vec<Message>, SyncError> {
        if headers.is_empty() {
            // No more headers, switch to downloading blocks or check if synced
            if !self.pending_headers.is_empty() {
                self.state = SyncState::DownloadingBlocks;
            } else if !self.blocks_in_flight.is_empty() {
                // Still waiting for blocks - stay in DownloadingBlocks
                self.state = SyncState::DownloadingBlocks;
            } else {
                self.state = SyncState::Synced;
            }
            self.synced_peers.insert(peer_id);
            return Ok(vec![]);
        }

        let blockchain = self.blockchain.read().await;
        let mut responses = Vec::new();

        // Validate first header connects to our chain
        let first = headers.first().ok_or(SyncError::EmptyHeaders)?;

        // Check if the first header connects to our blockchain or pending headers
        let connects_to_chain = blockchain.has_block(&first.prev_hash);
        let connects_to_pending = self
            .pending_headers
            .iter()
            .any(|h| h.hash() == first.prev_hash);
        let connects_to_downloaded = self.downloaded_headers.contains_key(&first.prev_hash);

        if !connects_to_chain && !connects_to_pending && !connects_to_downloaded {
            // This header doesn't connect to anything we know about
            // This could be a fork from an unknown point or a different chain entirely
            return Err(SyncError::ForkFromUnknownAncestor);
        }

        // Update peer state
        let peer_state = self.peer_states.entry(peer_id).or_insert_with(PeerSyncState::new);

        // Validate header chain
        let mut prev_hash = first.prev_hash;

        for header in &headers {
            // Verify chain continuity
            if header.prev_hash != prev_hash {
                return Err(SyncError::HeaderChainBroken);
            }

            // Verify proof of work
            if !header.check_pow() {
                return Err(SyncError::InvalidProofOfWork);
            }

            let header_hash = header.hash();
            prev_hash = header_hash;

            // Track that this peer knows about this header
            peer_state.known_headers.insert(header_hash);
        }

        // Update peer's tip info
        if let Some(last) = headers.last() {
            let last_hash = last.hash();
            peer_state.tip_hash = last_hash;
            // We don't know the exact height, but we can estimate
            peer_state.tip_height = peer_state.tip_height.saturating_add(headers.len() as u64);
        }

        drop(blockchain);

        // Add valid headers to pending queue and track them
        for header in headers {
            if self.pending_headers.len() >= MAX_PENDING_HEADERS {
                break;
            }
            let header_hash = header.hash();
            self.downloaded_headers.insert(header_hash, header.clone());
            self.pending_headers.push_back(header);
        }

        // If we got a full batch, request more
        if self.pending_headers.len() < MAX_PENDING_HEADERS {
            self.state = SyncState::DownloadingHeaders;
            responses.push(self.create_get_headers_message().await);
        } else {
            self.state = SyncState::DownloadingBlocks;
        }

        Ok(responses)
    }

    /// Get the next batch of blocks to download.
    pub fn get_blocks_to_download(&mut self, peer_id: u64) -> Option<Message> {
        if self.blocks_in_flight.len() >= MAX_BLOCKS_IN_FLIGHT {
            return None;
        }

        let mut items = Vec::new();
        let now = Instant::now();

        while let Some(header) = self.pending_headers.front() {
            let hash = header.hash();

            // Skip if already downloading
            if self.blocks_in_flight.contains_key(&hash) {
                self.pending_headers.pop_front();
                continue;
            }

            items.push(InvItem::block(hash));
            self.blocks_in_flight.insert(
                hash,
                BlockDownload {
                    peer_id,
                    requested_at: now,
                },
            );
            self.pending_headers.pop_front();

            if items.len() >= MAX_BLOCKS_IN_FLIGHT - self.blocks_in_flight.len() + items.len() {
                break;
            }
        }

        if items.is_empty() {
            None
        } else {
            Some(Message::GetData { items })
        }
    }

    /// Process a received block.
    pub async fn process_block(
        &mut self,
        peer_id: u64,
        block: Block,
    ) -> Result<bool, SyncError> {
        let hash = block.hash();

        // Remove from in-flight
        self.blocks_in_flight.remove(&hash);

        // Also remove from downloaded_headers now that we have the full block
        self.downloaded_headers.remove(&hash);

        // Try to add to blockchain - scope the lock to release it before orphan handling
        let result = {
            let mut blockchain = self.blockchain.write().await;
            match blockchain.add_block(block.clone()) {
                Ok(_) => {
                    tracing::info!(
                        hash = %hash,
                        height = blockchain.height(),
                        "added block to chain"
                    );
                    Ok(true)
                }
                Err(e) => Err(e),
            }
        }; // blockchain lock is dropped here

        match result {
            Ok(true) => {
                // Try to process any orphans that depend on this block
                self.process_orphans(hash).await;

                // Check if we're done syncing
                if self.pending_headers.is_empty() && self.blocks_in_flight.is_empty() {
                    self.state = SyncState::Synced;
                }

                Ok(true)
            }
            Err(BlockchainError::UnknownPreviousBlock) => {
                // Store as orphan with LRU eviction, tracking the originating peer
                tracing::debug!(hash = %hash, peer_id = peer_id, "storing orphan block");
                self.add_orphan(hash, block, peer_id);
                Ok(false)
            }
            Err(e) => {
                tracing::warn!(
                    hash = %hash,
                    error = %e,
                    peer_id = peer_id,
                    "block validation failed"
                );
                Err(SyncError::BlockValidationFailed(e))
            }
            Ok(false) => Ok(false), // Should not happen, but handle gracefully
        }
    }

    /// Try to process orphan blocks that may now be valid.
    async fn process_orphans(&mut self, new_hash: Hash) {
        // Find orphans that have this block as their parent
        let orphan_data: Vec<(Hash, u64)> = self
            .orphan_blocks
            .iter()
            .filter(|(_, (block, _))| block.header.prev_hash == new_hash)
            .map(|(hash, (_, peer_id))| (*hash, *peer_id))
            .collect();

        for (hash, peer_id) in orphan_data {
            if let Some((block, _)) = self.remove_orphan(&hash) {
                // Recursively try to add this orphan with its original peer_id
                if let Ok(true) = Box::pin(self.process_block(peer_id, block)).await {
                    // Successfully added, process its orphans too
                    Box::pin(self.process_orphans(hash)).await;
                }
            }
        }
    }

    /// Handle a block request timeout.
    pub fn handle_timeout(&mut self, hash: &Hash) -> Option<u64> {
        if let Some(download) = self.blocks_in_flight.remove(hash) {
            // Re-queue for download if we have the header
            if let Some(header) = self.downloaded_headers.get(hash).cloned() {
                self.pending_headers.push_front(header);
            }
            Some(download.peer_id)
        } else {
            None
        }
    }

    /// Check for timed out block requests and return the list of (peer_id, hash) pairs.
    ///
    /// This should be called periodically to detect stalled downloads.
    pub fn check_timeouts(&mut self) -> Vec<(u64, Hash)> {
        let now = Instant::now();

        let timed_out: Vec<(Hash, BlockDownload)> = self
            .blocks_in_flight
            .iter()
            .filter(|(_, d)| now.duration_since(d.requested_at) > BLOCK_DOWNLOAD_TIMEOUT)
            .map(|(h, d)| (*h, d.clone()))
            .collect();

        let mut results = Vec::new();
        for (hash, download) in timed_out {
            self.blocks_in_flight.remove(&hash);

            // Re-queue the block for download from another peer
            if let Some(header) = self.downloaded_headers.get(&hash).cloned() {
                self.pending_headers.push_front(header);
                tracing::debug!(
                    hash = %hash,
                    peer_id = download.peer_id,
                    "block download timed out, re-queuing"
                );
            }

            results.push((download.peer_id, hash));
        }

        results
    }

    /// Handle a peer disconnection.
    pub fn handle_peer_disconnected(&mut self, peer_id: u64) {
        // Re-queue any blocks this peer was downloading
        let to_requeue: Vec<(Hash, BlockDownload)> = self
            .blocks_in_flight
            .iter()
            .filter(|(_, d)| d.peer_id == peer_id)
            .map(|(h, d)| (*h, d.clone()))
            .collect();

        for (hash, _) in to_requeue {
            self.blocks_in_flight.remove(&hash);

            // Re-queue for download from another peer using stored header
            if let Some(header) = self.downloaded_headers.get(&hash).cloned() {
                self.pending_headers.push_front(header);
                tracing::debug!(
                    hash = %hash,
                    peer_id = peer_id,
                    "re-queuing block after peer disconnect"
                );
            }
        }

        // Remove peer's sync state
        self.peer_states.remove(&peer_id);
        self.synced_peers.remove(&peer_id);

        // Clear active sync peer if it was this peer
        if self.active_sync_peer == Some(peer_id) {
            self.active_sync_peer = None;
        }

        if self.best_peer == Some(peer_id) {
            self.best_peer = None;
        }
    }

    /// Create inventory message for a new block.
    pub fn create_block_inv(&self, block: &Block) -> Message {
        Message::Inv {
            items: vec![InvItem::block(block.hash())],
        }
    }

    /// Create inventory message for a new transaction.
    pub fn create_tx_inv(&self, tx: &crate::blockchain::Transaction) -> Message {
        Message::Inv {
            items: vec![InvItem::tx(tx.txid())],
        }
    }

    /// Process an inventory message.
    ///
    /// The `mempool_contains` closure checks if a transaction is already in the mempool.
    pub async fn process_inv<F>(&mut self, items: &[InvItem], mempool_contains: F) -> Option<Message>
    where
        F: Fn(&Hash) -> bool,
    {
        let blockchain = self.blockchain.read().await;
        let mut wanted = Vec::new();

        for item in items {
            match item.inv_type {
                InvType::Block => {
                    if !blockchain.has_block(&item.hash)
                        && !self.blocks_in_flight.contains_key(&item.hash)
                        && !self.orphan_blocks.contains_key(&item.hash)
                    {
                        wanted.push(item.clone());
                    }
                }
                InvType::Tx => {
                    // Check if we already have this transaction in mempool or blockchain
                    if !mempool_contains(&item.hash) && !blockchain.has_transaction(&item.hash) {
                        wanted.push(item.clone());
                    }
                }
            }
        }

        if wanted.is_empty() {
            None
        } else {
            Some(Message::GetData { items: wanted })
        }
    }

    /// Respond to a GetHeaders request.
    pub async fn respond_to_get_headers(
        &self,
        locator: &[Hash],
        stop: &Hash,
    ) -> Message {
        let blockchain = self.blockchain.read().await;
        let mut headers = Vec::new();

        // Find the first locator hash we have
        let start_height = locator
            .iter()
            .find_map(|hash| blockchain.height_of(hash))
            .map(|h| h + 1)
            .unwrap_or(0);

        // Collect headers starting from that point
        let mut height = start_height;
        while headers.len() < MAX_HEADERS_COUNT {
            if let Some(hash) = blockchain.hash_at_height(height) {
                if let Some(block) = blockchain.get_block(&hash) {
                    headers.push(block.header);
                    if hash == *stop {
                        break;
                    }
                }
            } else {
                break;
            }
            height += 1;
        }

        Message::Headers { headers }
    }

    /// Respond to a GetData request.
    ///
    /// The `mempool_get` closure retrieves a transaction from the mempool by hash.
    pub async fn respond_to_get_data<F>(
        &self,
        items: &[InvItem],
        mempool_get: F,
    ) -> Vec<Message>
    where
        F: Fn(&Hash) -> Option<crate::blockchain::Transaction>,
    {
        let blockchain = self.blockchain.read().await;
        let mut responses = Vec::new();

        for item in items {
            match item.inv_type {
                InvType::Block => {
                    if let Some(block) = blockchain.get_block(&item.hash) {
                        responses.push(Message::Block(block.clone()));
                    }
                }
                InvType::Tx => {
                    // Check mempool for this transaction
                    if let Some(tx) = mempool_get(&item.hash) {
                        responses.push(Message::Tx(tx));
                    }
                    // Could also check blockchain for confirmed txs if needed
                }
            }
        }

        responses
    }
}

/// Errors that can occur during synchronization.
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("unknown previous header")]
    UnknownPreviousHeader,
    #[error("header chain is broken")]
    HeaderChainBroken,
    #[error("invalid proof of work")]
    InvalidProofOfWork,
    #[error("block validation failed: {0}")]
    BlockValidationFailed(BlockchainError),
    #[error("empty headers message")]
    EmptyHeaders,
    #[error("fork from unknown ancestor - headers don't connect to our chain")]
    ForkFromUnknownAncestor,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blockchain::{create_genesis_block, Address};
    use crate::crypto::ml_dsa_87;

    fn create_test_blockchain() -> Arc<RwLock<Blockchain>> {
        let (pk, _) = ml_dsa_87::keygen();
        let addr = Address::from_public_key(&pk);
        let genesis = create_genesis_block(0, 0x40ffffff, 50_000_000, addr);
        let blockchain = Blockchain::new(genesis, 2016, 600, 50_000_000, 210_000);
        Arc::new(RwLock::new(blockchain))
    }

    #[tokio::test]
    async fn test_build_locator() {
        let blockchain = create_test_blockchain();
        let sync = SyncManager::new(blockchain);
        let locator = sync.build_locator().await;
        assert!(!locator.is_empty());
    }

    #[tokio::test]
    async fn test_sync_state_transitions() {
        let blockchain = create_test_blockchain();
        let mut sync = SyncManager::new(blockchain);
        assert_eq!(sync.state(), SyncState::Idle);

        // Empty headers response should transition to synced
        sync.process_headers(1, vec![]).await.unwrap();
        assert_eq!(sync.state(), SyncState::Synced);
    }
}
