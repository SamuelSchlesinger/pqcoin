//! Message handling logic for the network service.
//!
//! This module contains the implementation of message handlers extracted from
//! the main NetworkService. These handle incoming protocol messages from peers.

use crate::blockchain::{Block, Blockchain, Transaction};
use crate::mempool::Mempool;
use crate::network::message::{InvItem, Message};
use crate::network::service::events::NetworkEvent;
use crate::network::service::state::{NetworkState, PeerCommand, PeerMessage};
use crate::network::sync::SyncManager;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

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
            handle_handshake_complete(peer_id, height, state).await;
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
) {
    let mut state = state.write().await;
    if let Some(info) = state.peers.get_mut(&peer_id) {
        info.height = height;
        tracing::debug!(peer_id = peer_id, height = height, "updated peer height");
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
            handle_addr(addrs, state).await;
        }

        Message::AddrV2 { addrs } => {
            handle_addr_v2(addrs, state).await;
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
    tracing::debug!(peer_id = peer_id, nonce = nonce, "received ping, sending pong");
    send_to_peer(peer_id, Message::Pong(nonce), state).await;
}

/// Handle a GetAddr message by sending known addresses.
async fn handle_get_addr(peer_id: u64, state: &Arc<RwLock<NetworkState>>) {
    let state_guard = state.read().await;
    let addrs: Vec<std::net::SocketAddr> = state_guard
        .known_addrs
        .iter()
        .take(1000)
        .cloned()
        .collect();
    drop(state_guard);
    send_to_peer(peer_id, Message::Addr { addrs }, state).await;
}

/// Handle an Addr message by storing new addresses.
async fn handle_addr(addrs: Vec<std::net::SocketAddr>, state: &Arc<RwLock<NetworkState>>) {
    let mut state_guard = state.write().await;
    for addr in addrs {
        if !state_guard.known_addrs.contains(&addr) {
            state_guard.known_addrs.push(addr);
        }
    }
}

/// Handle an AddrV2 message by storing new addresses (filtering stale ones).
async fn handle_addr_v2(
    addrs: Vec<crate::network::message::TimestampedAddr>,
    state: &Arc<RwLock<NetworkState>>,
) {
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut state_guard = state.write().await;
    for taddr in addrs {
        if !taddr.is_stale(current_time) && !state_guard.known_addrs.contains(&taddr.addr) {
            state_guard.known_addrs.push(taddr.addr);
        }
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
    if let Some(response) = sync_guard.process_inv(&items, |hash| mempool_guard.contains(hash)).await {
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
    let responses = sync_guard.respond_to_get_data(&items, |hash| mempool_guard.get(hash).cloned()).await;
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
            broadcast_except(peer_id, Message::Inv {
                items: vec![InvItem::block(block_hash)],
            }, state).await;
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
            broadcast_except(peer_id, Message::Inv {
                items: vec![InvItem::tx(txid)],
            }, state).await;
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

        if let Some(info) = &info {
            state_guard.connected_addrs.remove(&info.addr);
            if info.outbound {
                state_guard.outbound_count = state_guard.outbound_count.saturating_sub(1);
            }
            // Update rate limiter and subnet limiter connection counts
            state_guard.rate_limiter.record_disconnection(info.addr.ip());
            state_guard.subnet_limiter.record_disconnection(&info.addr.ip());
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
pub(crate) async fn send_to_peer(peer_id: u64, message: Message, state: &Arc<RwLock<NetworkState>>) {
    let state_guard = state.read().await;
    if let Some(sender) = state_guard.peer_senders.get(&peer_id) {
        let _ = sender.send(PeerCommand::Send(message)).await;
    }
}

/// Broadcast a message to all peers except one.
pub(crate) async fn broadcast_except(except_peer_id: u64, message: Message, state: &Arc<RwLock<NetworkState>>) {
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
