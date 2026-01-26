//! Address manager for peer discovery.
//!
//! This module implements Bitcoin Core-style peer discovery with a two-bucket
//! system (new/tried) and random peer selection for network resilience.

use crate::constants::{ADDR_MAX_AGE_SECS, FAILED_ADDR_COOLDOWN_SECS, MAX_KNOWN_ADDRS};
use crate::network::message::{Services, TimestampedAddr};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::time::Instant;

/// Metadata for a known peer address.
#[derive(Debug, Clone)]
pub struct AddrEntry {
    /// The socket address.
    pub addr: SocketAddr,
    /// Services offered by this peer.
    pub services: Services,
    /// Unix timestamp when this address was last seen active.
    pub last_seen: u64,
    /// Unix timestamp of last connection attempt.
    pub last_attempt: Option<u64>,
    /// Number of connection attempts.
    pub attempt_count: u32,
    /// Whether we've successfully connected to this address before.
    pub tried: bool,
}

/// Address manager for peer discovery.
///
/// Uses a two-bucket system:
/// - "new" bucket: Addresses we've heard about but never successfully connected to
/// - "tried" bucket: Addresses we've successfully connected to before
///
/// Selection prefers tried addresses (70%) over new addresses (30%) to improve
/// connection success rate while still discovering new peers.
pub struct AddressManager {
    /// All known addresses.
    addrs: HashMap<SocketAddr, AddrEntry>,
    /// Untried addresses (FIFO queue for fairness).
    new_addrs: VecDeque<SocketAddr>,
    /// Successfully connected addresses.
    tried_addrs: VecDeque<SocketAddr>,
    /// Currently attempting connection.
    in_progress: HashSet<SocketAddr>,
    /// Failed addresses with cooldown expiry.
    failed_cooldown: HashMap<SocketAddr, Instant>,
    /// Our own addresses (avoid self-connection).
    local_addrs: HashSet<SocketAddr>,
}

impl AddressManager {
    /// Create a new address manager.
    pub fn new() -> Self {
        Self {
            addrs: HashMap::new(),
            new_addrs: VecDeque::new(),
            tried_addrs: VecDeque::new(),
            in_progress: HashSet::new(),
            failed_cooldown: HashMap::new(),
            local_addrs: HashSet::new(),
        }
    }

    /// Add a local address to avoid self-connections.
    pub fn add_local_addr(&mut self, addr: SocketAddr) {
        self.local_addrs.insert(addr);
    }

    /// Add a new address to the manager.
    ///
    /// Returns true if the address was added (new), false if rejected or already known.
    pub fn add_addr(
        &mut self,
        addr: SocketAddr,
        services: Services,
        timestamp: u64,
        _source: Option<SocketAddr>,
    ) -> bool {
        // Reject local addresses (self-connection prevention)
        if self.local_addrs.contains(&addr) {
            return false;
        }

        // Reject obviously invalid addresses
        if addr.ip().is_loopback() || addr.ip().is_unspecified() {
            return false;
        }

        // Check if already at capacity
        if self.addrs.len() >= MAX_KNOWN_ADDRS && !self.addrs.contains_key(&addr) {
            // Evict oldest new address to make room
            if let Some(old_addr) = self.new_addrs.pop_front() {
                self.addrs.remove(&old_addr);
            } else {
                return false;
            }
        }

        // Check for stale addresses
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        if current_time.saturating_sub(timestamp) > ADDR_MAX_AGE_SECS {
            return false;
        }

        // Update or insert
        if let Some(entry) = self.addrs.get_mut(&addr) {
            // Update existing entry with newer timestamp
            if timestamp > entry.last_seen {
                entry.last_seen = timestamp;
                entry.services = services;
            }
            false // Not new
        } else {
            // Insert new entry
            self.addrs.insert(
                addr,
                AddrEntry {
                    addr,
                    services,
                    last_seen: timestamp,
                    last_attempt: None,
                    attempt_count: 0,
                    tried: false,
                },
            );
            self.new_addrs.push_back(addr);
            true
        }
    }

    /// Add multiple addresses from a peer.
    ///
    /// Returns the number of new addresses added.
    pub fn add_addrs(&mut self, addrs: Vec<TimestampedAddr>, source: SocketAddr) -> usize {
        let mut added = 0;
        for taddr in addrs {
            if self.add_addr(taddr.addr, taddr.services, taddr.timestamp, Some(source)) {
                added += 1;
            }
        }
        added
    }

    /// Mark an address as successfully connected (move to tried bucket).
    pub fn mark_good(&mut self, addr: &SocketAddr) {
        self.in_progress.remove(addr);
        self.failed_cooldown.remove(addr);

        if let Some(entry) = self.addrs.get_mut(addr) {
            if !entry.tried {
                entry.tried = true;
                // Remove from new_addrs and add to tried_addrs
                self.new_addrs.retain(|a| a != addr);
                self.tried_addrs.push_back(*addr);
            }

            // Update last seen time
            entry.last_seen = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            entry.attempt_count = 0;
        }
    }

    /// Mark a connection attempt as failed.
    pub fn mark_attempt_failed(&mut self, addr: &SocketAddr) {
        self.in_progress.remove(addr);
        self.failed_cooldown.insert(
            *addr,
            Instant::now() + std::time::Duration::from_secs(FAILED_ADDR_COOLDOWN_SECS),
        );

        if let Some(entry) = self.addrs.get_mut(addr) {
            entry.attempt_count += 1;
            entry.last_attempt = Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            );
        }
    }

    /// Mark an address as currently being connected to.
    pub fn mark_in_progress(&mut self, addr: &SocketAddr) {
        self.in_progress.insert(*addr);
    }

    /// Clear in-progress status for an address.
    #[allow(dead_code)]
    pub fn clear_in_progress(&mut self, addr: &SocketAddr) {
        self.in_progress.remove(addr);
    }

    /// Get a random address for connection.
    ///
    /// Uses a 70/30 split between tried and new addresses.
    /// Respects cooldowns and excludes addresses in the exclude set.
    pub fn get_random_addr(&mut self, exclude: &HashSet<SocketAddr>) -> Option<SocketAddr> {
        let now = Instant::now();

        // Clean up expired cooldowns
        self.failed_cooldown.retain(|_, expiry| *expiry > now);

        // 70% chance to try from tried bucket, 30% from new
        let use_tried = (rand::random::<u8>() % 10) < 7 && !self.tried_addrs.is_empty();

        let candidates: Vec<SocketAddr> = if use_tried {
            self.tried_addrs
                .iter()
                .filter(|addr| {
                    !exclude.contains(*addr)
                        && !self.in_progress.contains(*addr)
                        && !self.failed_cooldown.contains_key(*addr)
                        && !self.local_addrs.contains(*addr)
                })
                .cloned()
                .collect()
        } else {
            self.new_addrs
                .iter()
                .filter(|addr| {
                    !exclude.contains(*addr)
                        && !self.in_progress.contains(*addr)
                        && !self.failed_cooldown.contains_key(*addr)
                        && !self.local_addrs.contains(*addr)
                })
                .cloned()
                .collect()
        };

        if candidates.is_empty() {
            // Try the other bucket
            let other_candidates: Vec<SocketAddr> = if use_tried {
                self.new_addrs
                    .iter()
                    .filter(|addr| {
                        !exclude.contains(*addr)
                            && !self.in_progress.contains(*addr)
                            && !self.failed_cooldown.contains_key(*addr)
                            && !self.local_addrs.contains(*addr)
                    })
                    .cloned()
                    .collect()
            } else {
                self.tried_addrs
                    .iter()
                    .filter(|addr| {
                        !exclude.contains(*addr)
                            && !self.in_progress.contains(*addr)
                            && !self.failed_cooldown.contains_key(*addr)
                            && !self.local_addrs.contains(*addr)
                    })
                    .cloned()
                    .collect()
            };

            if other_candidates.is_empty() {
                None
            } else {
                let idx = rand::random::<usize>() % other_candidates.len();
                Some(other_candidates[idx])
            }
        } else {
            let idx = rand::random::<usize>() % candidates.len();
            Some(candidates[idx])
        }
    }

    /// Get addresses for relay to other peers.
    ///
    /// Prefers recently seen addresses.
    pub fn get_addrs_for_relay(&self, max_count: usize) -> Vec<TimestampedAddr> {
        let current_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Collect all addresses with their entries
        let mut entries: Vec<&AddrEntry> = self.addrs.values().collect();

        // Sort by last_seen (most recent first)
        entries.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));

        // Take up to max_count
        entries
            .into_iter()
            .take(max_count)
            .filter(|entry| current_time.saturating_sub(entry.last_seen) <= ADDR_MAX_AGE_SECS)
            .map(|entry| TimestampedAddr {
                timestamp: entry.last_seen,
                services: entry.services,
                addr: entry.addr,
            })
            .collect()
    }

    /// Get the total number of known addresses.
    pub fn addr_count(&self) -> usize {
        self.addrs.len()
    }

    /// Get the number of new (untried) addresses.
    #[allow(dead_code)]
    pub fn new_addr_count(&self) -> usize {
        self.new_addrs.len()
    }

    /// Get the number of tried addresses.
    #[allow(dead_code)]
    pub fn tried_addr_count(&self) -> usize {
        self.tried_addrs.len()
    }

    /// Remove stale addresses older than ADDR_MAX_AGE_SECS.
    #[allow(dead_code)]
    pub fn cleanup_stale(&mut self, current_time: u64) {
        let stale: Vec<SocketAddr> = self
            .addrs
            .iter()
            .filter(|(_, entry)| current_time.saturating_sub(entry.last_seen) > ADDR_MAX_AGE_SECS)
            .map(|(addr, _)| *addr)
            .collect();

        for addr in stale {
            self.addrs.remove(&addr);
            self.new_addrs.retain(|a| a != &addr);
            self.tried_addrs.retain(|a| a != &addr);
        }
    }
}

impl Default for AddressManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn make_addr(port: u16) -> SocketAddr {
        SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(192, 168, 1, port as u8),
            port,
        ))
    }

    fn current_timestamp() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn test_add_addr() {
        let mut addrman = AddressManager::new();
        let addr = make_addr(8333);
        let now = current_timestamp();

        assert!(addrman.add_addr(addr, Services::NODE_NETWORK, now, None));
        assert_eq!(addrman.addr_count(), 1);
        assert_eq!(addrman.new_addr_count(), 1);
        assert_eq!(addrman.tried_addr_count(), 0);

        // Adding same address again should return false
        assert!(!addrman.add_addr(addr, Services::NODE_NETWORK, now, None));
        assert_eq!(addrman.addr_count(), 1);
    }

    #[test]
    fn test_reject_local_addr() {
        let mut addrman = AddressManager::new();
        let local_addr = make_addr(8333);
        addrman.add_local_addr(local_addr);

        let now = current_timestamp();
        assert!(!addrman.add_addr(local_addr, Services::NODE_NETWORK, now, None));
        assert_eq!(addrman.addr_count(), 0);
    }

    #[test]
    fn test_reject_stale_addr() {
        let mut addrman = AddressManager::new();
        let addr = make_addr(8333);
        let stale_time = current_timestamp() - ADDR_MAX_AGE_SECS - 1;

        assert!(!addrman.add_addr(addr, Services::NODE_NETWORK, stale_time, None));
        assert_eq!(addrman.addr_count(), 0);
    }

    #[test]
    fn test_mark_good() {
        let mut addrman = AddressManager::new();
        let addr = make_addr(8333);
        let now = current_timestamp();

        addrman.add_addr(addr, Services::NODE_NETWORK, now, None);
        assert_eq!(addrman.new_addr_count(), 1);
        assert_eq!(addrman.tried_addr_count(), 0);

        addrman.mark_good(&addr);
        assert_eq!(addrman.new_addr_count(), 0);
        assert_eq!(addrman.tried_addr_count(), 1);
    }

    #[test]
    fn test_get_random_addr() {
        let mut addrman = AddressManager::new();
        let now = current_timestamp();

        for port in 8333..8343 {
            addrman.add_addr(make_addr(port), Services::NODE_NETWORK, now, None);
        }

        let exclude = HashSet::new();
        let addr = addrman.get_random_addr(&exclude);
        assert!(addr.is_some());
    }

    #[test]
    fn test_get_random_addr_respects_exclude() {
        let mut addrman = AddressManager::new();
        let now = current_timestamp();
        let addr = make_addr(8333);

        addrman.add_addr(addr, Services::NODE_NETWORK, now, None);

        let mut exclude = HashSet::new();
        exclude.insert(addr);

        let result = addrman.get_random_addr(&exclude);
        assert!(result.is_none());
    }

    #[test]
    fn test_get_addrs_for_relay() {
        let mut addrman = AddressManager::new();
        let now = current_timestamp();

        for port in 8333..8343 {
            addrman.add_addr(make_addr(port), Services::NODE_NETWORK, now, None);
        }

        let relay_addrs = addrman.get_addrs_for_relay(5);
        assert_eq!(relay_addrs.len(), 5);
    }

    #[test]
    fn test_cleanup_stale() {
        let mut addrman = AddressManager::new();
        let now = current_timestamp();
        let old_time = now - ADDR_MAX_AGE_SECS / 2; // Recent enough to add

        addrman.add_addr(make_addr(8333), Services::NODE_NETWORK, old_time, None);
        assert_eq!(addrman.addr_count(), 1);

        // Cleanup with future time should remove the address
        addrman.cleanup_stale(now + ADDR_MAX_AGE_SECS);
        assert_eq!(addrman.addr_count(), 0);
    }
}
