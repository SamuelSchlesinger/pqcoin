//! Message handling logic for the network service.
//!
//! This module contains the implementation of message handlers extracted from
//! the main NetworkService. These handle incoming protocol messages from peers.

use crate::blockchain::{Block, Blockchain, Transaction};
use crate::mempool::Mempool;
use crate::network::message::{InvItem, Message, Services};
use crate::network::service::events::NetworkEvent;
use crate::network::service::state::{NetworkState, PeerCommand, PeerMessage};
use crate::network::sync::SyncManager;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{RwLock, mpsc};

/// Handles a message from a peer task by dispatching to the appropriate handler.
pub(crate) async fn handle_peer_message(
    msg: PeerMessage,
    state: &Arc<RwLock<NetworkState>>,
    blockchain: &Arc<RwLock<Blockchain>>,
    mempool: &Arc<RwLock<Mempool>>,
    sync: &Arc<RwLock<SyncManager>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) {
    match msg {
        PeerMessage::HandshakeComplete { peer_id, height } => {
            handle_handshake_complete(peer_id, height, state, sync).await;
        }
        PeerMessage::Received { peer_id, message } => {
            handle_message(peer_id, message, state, blockchain, mempool, sync, event_tx).await;
        }
        PeerMessage::Disconnected { peer_id, error } => {
            handle_disconnect(peer_id, error, state, sync, event_tx).await;
        }
    }
}

/// Handle a successful handshake completion.
async fn handle_handshake_complete(
    peer_id: u64,
    height: u64,
    state: &Arc<RwLock<NetworkState>>,
    sync: &Arc<RwLock<SyncManager>>,
) {
    {
        let mut state_guard = state.write().await;
        if let Some(info) = state_guard.peers.get_mut(&peer_id) {
            info.height = height;
            let addr = info.addr;
            tracing::debug!(peer_id = peer_id, height = height, "updated peer height");

            // Mark address as good in address manager
            state_guard.addr_manager.mark_good(&addr);

            // Schedule GetAddr after delay
            state_guard.pending_getaddr.insert(peer_id, Instant::now());
        }
    }

    // Try to start sync immediately if this peer is ahead of us
    // This avoids waiting for the next sync interval tick
    let get_headers_msg = {
        let mut sync_guard = sync.write().await;
        sync_guard.maybe_start_sync(peer_id, height).await
    };

    // Send GetHeaders if sync was started
    if let Some(msg) = get_headers_msg {
        send_to_peer(peer_id, msg, state).await;
    }
}

/// Handle a message from a peer.
pub(crate) async fn handle_message(
    peer_id: u64,
    message: Message,
    state: &Arc<RwLock<NetworkState>>,
    blockchain: &Arc<RwLock<Blockchain>>,
    mempool: &Arc<RwLock<Mempool>>,
    sync: &Arc<RwLock<SyncManager>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) {
    match message {
        Message::Ping(nonce) => {
            handle_ping(peer_id, nonce, state).await;
        }

        Message::Pong(nonce) => {
            tracing::debug!(peer_id = peer_id, nonce = nonce, "received pong");
        }

        Message::GetAddr => {
            handle_get_addr(peer_id, state).await;
        }

        Message::Addr { addrs } => {
            handle_addr(peer_id, addrs, state).await;
        }

        Message::AddrV2 { addrs } => {
            handle_addr_v2(peer_id, addrs, state).await;
        }

        Message::SendHeaders => {
            // Peer requests block announcements via headers
            // TODO: Track this preference per-peer and use Headers instead of Inv
            tracing::debug!(peer_id = peer_id, "peer requested SendHeaders mode");
        }

        Message::Inv { items } => {
            handle_inv(peer_id, items, state, mempool, sync).await;
        }

        Message::GetData { items } => {
            handle_get_data(peer_id, items, state, mempool, sync).await;
        }

        Message::GetHeaders { locator, stop } => {
            handle_get_headers(peer_id, locator, stop, state, sync).await;
        }

        Message::Headers { headers } => {
            handle_headers(peer_id, headers, state, sync).await;
        }

        Message::Block(block) => {
            handle_block(peer_id, block, state, mempool, sync, event_tx).await;
        }

        Message::Tx(tx) => {
            handle_tx(peer_id, tx, state, blockchain, mempool, event_tx).await;
        }

        Message::Reject { message, reason } => {
            tracing::warn!(
                peer_id = peer_id,
                message = message,
                reason = reason,
                "peer rejected message"
            );
        }

        Message::Version { .. } | Message::Verack => {
            // Should not receive these after handshake
            tracing::warn!(peer_id = peer_id, "unexpected handshake message");
        }
    }
}

/// Handle a ping message by sending a pong.
async fn handle_ping(peer_id: u64, nonce: u64, state: &Arc<RwLock<NetworkState>>) {
    tracing::debug!(
        peer_id = peer_id,
        nonce = nonce,
        "received ping, sending pong"
    );
    send_to_peer(peer_id, Message::Pong(nonce), state).await;
}

/// Minimum interval between GetAddr responses to same peer (seconds).
const GETADDR_RESPONSE_INTERVAL_SECS: u64 = 60;

/// Handle a GetAddr message by sending known addresses.
/// Rate-limited to prevent bandwidth amplification attacks.
async fn handle_get_addr(peer_id: u64, state: &Arc<RwLock<NetworkState>>) {
    let now = std::time::Instant::now();

    // Check rate limit and update last response time
    let should_respond = {
        let mut state_guard = state.write().await;
        if let Some(last_time) = state_guard.last_getaddr_response.get(&peer_id) {
            if now.duration_since(*last_time)
                < std::time::Duration::from_secs(GETADDR_RESPONSE_INTERVAL_SECS)
            {
                tracing::debug!(peer_id = peer_id, "rate limiting GetAddr response");
                return;
            }
        }
        state_guard.last_getaddr_response.insert(peer_id, now);
        true
    };

    if !should_respond {
        return;
    }

    let addrs = {
        let state_guard = state.read().await;
        state_guard.addr_manager.get_addrs_for_relay(1000)
    };
    tracing::debug!(
        peer_id = peer_id,
        count = addrs.len(),
        "responding to GetAddr"
    );
    send_to_peer(peer_id, Message::AddrV2 { addrs }, state).await;
}

/// Handle an Addr message by storing new addresses.
async fn handle_addr(
    peer_id: u64,
    addrs: Vec<std::net::SocketAddr>,
    state: &Arc<RwLock<NetworkState>>,
) {
    let source = {
        let state_guard = state.read().await;
        state_guard.peers.get(&peer_id).map(|p| p.addr)
    };

    // INVARIANT: SystemTime::now() is always after UNIX_EPOCH.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let mut added = 0;
    {
        let mut state_guard = state.write().await;
        for addr in addrs {
            if state_guard
                .addr_manager
                .add_addr(addr, Services::NODE_NETWORK, now, source)
            {
                added += 1;
            }
        }
    }

    if added > 0 {
        tracing::debug!(
            peer_id = peer_id,
            added = added,
            "added addresses from Addr message"
        );
    }
}

/// Handle an AddrV2 message by storing new addresses (filtering stale ones).
async fn handle_addr_v2(
    peer_id: u64,
    addrs: Vec<crate::network::message::TimestampedAddr>,
    state: &Arc<RwLock<NetworkState>>,
) {
    // Get source address, returning early if peer disconnected
    let source = {
        let state_guard = state.read().await;
        match state_guard.peers.get(&peer_id) {
            Some(p) => p.addr,
            None => {
                tracing::debug!(peer_id = peer_id, "ignoring AddrV2 from disconnected peer");
                return;
            }
        }
    };

    let added = {
        let mut state_guard = state.write().await;
        state_guard.addr_manager.add_addrs(addrs, source)
    };

    if added > 0 {
        tracing::debug!(
            peer_id = peer_id,
            added = added,
            "added addresses from AddrV2 message"
        );
    }
}

/// Handle an Inv message by requesting unknown items.
async fn handle_inv(
    peer_id: u64,
    items: Vec<InvItem>,
    state: &Arc<RwLock<NetworkState>>,
    mempool: &Arc<RwLock<Mempool>>,
    sync: &Arc<RwLock<SyncManager>>,
) {
    let mempool_guard = mempool.read().await;
    let mut sync_guard = sync.write().await;
    if let Some(response) = sync_guard
        .process_inv(&items, |hash| mempool_guard.contains(hash))
        .await
    {
        drop(sync_guard);
        drop(mempool_guard);
        send_to_peer(peer_id, response, state).await;
    }
}

/// Handle a GetData message by sending requested items.
async fn handle_get_data(
    peer_id: u64,
    items: Vec<InvItem>,
    state: &Arc<RwLock<NetworkState>>,
    mempool: &Arc<RwLock<Mempool>>,
    sync: &Arc<RwLock<SyncManager>>,
) {
    let mempool_guard = mempool.read().await;
    let sync_guard = sync.read().await;
    let responses = sync_guard
        .respond_to_get_data(&items, |hash| mempool_guard.get(hash).cloned())
        .await;
    drop(sync_guard);
    drop(mempool_guard);
    for response in responses {
        send_to_peer(peer_id, response, state).await;
    }
}

/// Handle a GetHeaders message by sending headers.
async fn handle_get_headers(
    peer_id: u64,
    locator: Vec<crate::crypto::Hash>,
    stop: crate::crypto::Hash,
    state: &Arc<RwLock<NetworkState>>,
    sync: &Arc<RwLock<SyncManager>>,
) {
    let sync_guard = sync.read().await;
    let response = sync_guard.respond_to_get_headers(&locator, &stop).await;
    drop(sync_guard);
    send_to_peer(peer_id, response, state).await;
}

/// Handle a Headers message by processing and potentially requesting blocks.
async fn handle_headers(
    peer_id: u64,
    headers: Vec<crate::blockchain::BlockHeader>,
    state: &Arc<RwLock<NetworkState>>,
    sync: &Arc<RwLock<SyncManager>>,
) {
    let mut sync_guard = sync.write().await;
    match sync_guard.process_headers(peer_id, headers).await {
        Ok(responses) => {
            drop(sync_guard);
            for response in responses {
                send_to_peer(peer_id, response, state).await;
            }
        }
        Err(e) => {
            tracing::warn!(peer_id = peer_id, error = %e, "header processing failed");
        }
    }
}

/// Handle a Block message by processing and potentially relaying it.
async fn handle_block(
    peer_id: u64,
    block: Block,
    state: &Arc<RwLock<NetworkState>>,
    mempool: &Arc<RwLock<Mempool>>,
    sync: &Arc<RwLock<SyncManager>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) {
    // First, process block with sync lock only
    let block_hash = block.hash();
    let added = {
        let mut sync_guard = sync.write().await;
        sync_guard.process_block(peer_id, block.clone()).await
    };

    match added {
        Ok(true) => {
            // Block was added, now update mempool separately
            {
                let mut mempool_guard = mempool.write().await;
                mempool_guard.remove_confirmed(&block.transactions);
                mempool_guard.remove_conflicts(&block.transactions);
            }

            // Events and broadcast without holding locks
            let _ = event_tx.send(NetworkEvent::NewBlock(block.clone())).await;
            broadcast_except(
                peer_id,
                Message::Inv {
                    items: vec![InvItem::block(block_hash)],
                },
                state,
            )
            .await;
        }
        Ok(false) => {
            // Orphan block, already stored
        }
        Err(e) => {
            tracing::warn!(peer_id = peer_id, error = %e, "block processing failed");
        }
    }
}

/// Handle a Tx message by validating and potentially relaying.
async fn handle_tx(
    peer_id: u64,
    tx: Transaction,
    state: &Arc<RwLock<NetworkState>>,
    blockchain: &Arc<RwLock<Blockchain>>,
    mempool: &Arc<RwLock<Mempool>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) {
    // Validate and add to mempool
    let txid = tx.txid();
    let blockchain_guard = blockchain.read().await;
    let current_height = blockchain_guard.height();
    let mut mempool_guard = mempool.write().await;

    match mempool_guard.add(tx.clone(), &blockchain_guard, current_height) {
        Ok(true) => {
            drop(mempool_guard);
            drop(blockchain_guard);

            tracing::debug!(
                txid = %txid.to_hex()[..16],
                "added transaction to mempool"
            );

            let _ = event_tx
                .send(NetworkEvent::NewTransaction(tx.clone()))
                .await;

            // Relay to other peers
            broadcast_except(
                peer_id,
                Message::Inv {
                    items: vec![InvItem::tx(txid)],
                },
                state,
            )
            .await;
        }
        Ok(false) => {
            // Already in mempool, ignore
            tracing::trace!(txid = %txid.to_hex()[..16], "transaction already in mempool");
        }
        Err(e) => {
            tracing::debug!(
                txid = %txid.to_hex()[..16],
                error = %e,
                "rejected transaction"
            );
        }
    }
}

/// Handle a peer disconnection.
pub(crate) async fn handle_disconnect(
    peer_id: u64,
    error: Option<String>,
    state: &Arc<RwLock<NetworkState>>,
    sync: &Arc<RwLock<SyncManager>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) {
    let addr = {
        let mut state_guard = state.write().await;
        let info = state_guard.peers.remove(&peer_id);
        state_guard.peer_senders.remove(&peer_id);
        state_guard.last_getaddr_response.remove(&peer_id);

        if let Some(info) = &info {
            state_guard.connected_addrs.remove(&info.addr);
            if info.outbound {
                state_guard.outbound_count = state_guard.outbound_count.saturating_sub(1);
            }
            // Update rate limiter and subnet limiter connection counts
            state_guard
                .rate_limiter
                .record_disconnection(info.addr.ip());
            state_guard
                .subnet_limiter
                .record_disconnection(&info.addr.ip());
        }

        info.map(|i| i.addr)
    };

    if let Some(addr) = addr {
        tracing::info!(
            peer_id = peer_id,
            addr = %addr,
            error = error.as_deref().unwrap_or("none"),
            "peer disconnected"
        );

        let _ = event_tx
            .send(NetworkEvent::PeerDisconnected { peer_id, addr })
            .await;

        // Notify sync manager
        let mut sync_guard = sync.write().await;
        sync_guard.handle_peer_disconnected(peer_id);
    }
}

/// Send a message to a specific peer.
pub(crate) async fn send_to_peer(
    peer_id: u64,
    message: Message,
    state: &Arc<RwLock<NetworkState>>,
) {
    let state_guard = state.read().await;
    if let Some(sender) = state_guard.peer_senders.get(&peer_id) {
        let _ = sender.send(PeerCommand::Send(message)).await;
    }
}

/// Broadcast a message to all peers except one.
pub(crate) async fn broadcast_except(
    except_peer_id: u64,
    message: Message,
    state: &Arc<RwLock<NetworkState>>,
) {
    let state_guard = state.read().await;
    for (&peer_id, sender) in &state_guard.peer_senders {
        if peer_id != except_peer_id {
            let _ = sender.send(PeerCommand::Send(message.clone())).await;
        }
    }
}

/// Broadcast a message to all peers.
pub(crate) async fn broadcast(message: Message, state: &Arc<RwLock<NetworkState>>) {
    let state_guard = state.read().await;
    for sender in state_guard.peer_senders.values() {
        let _ = sender.send(PeerCommand::Send(message.clone())).await;
    }
}
