//! Network defense mechanisms.
//!
//! This module provides protection against various network-level attacks:
//! - `BanList`: Tracks and enforces bans on misbehaving peers
//! - `RateLimiter`: Prevents DoS attacks by limiting connection attempts
//! - `SubnetLimiter`: Protects against eclipse attacks by limiting connections per subnet

use crate::constants::{
    BAN_DURATION_SECS, CONNECTION_RATE_LIMIT_SECS, MAX_CONNECTIONS_PER_IP, MAX_PER_SUBNET,
};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

/// Ban list for misbehaving peers.
///
/// Tracks banned IP addresses with expiration times.
pub(crate) struct BanList {
    /// Banned IPs with their ban expiry time.
    banned_ips: HashMap<IpAddr, Instant>,
}

impl BanList {
    pub(crate) fn new() -> Self {
        Self {
            banned_ips: HashMap::new(),
        }
    }

    /// Check if an IP is currently banned.
    pub(crate) fn is_banned(&self, ip: &IpAddr) -> bool {
        if let Some(expiry) = self.banned_ips.get(ip) {
            Instant::now() < *expiry
        } else {
            false
        }
    }

    /// Ban an IP for the configured duration.
    pub(crate) fn ban(&mut self, ip: IpAddr) {
        let expiry = Instant::now() + Duration::from_secs(BAN_DURATION_SECS);
        self.banned_ips.insert(ip, expiry);
        tracing::info!(ip = %ip, duration_secs = BAN_DURATION_SECS, "banned peer");
    }

    /// Clean up expired bans.
    #[allow(dead_code)]
    pub(crate) fn cleanup(&mut self) {
        let now = Instant::now();
        self.banned_ips.retain(|_, expiry| now < *expiry);
    }
}

/// Rate limiter for incoming connections.
///
/// Prevents DoS attacks by limiting connection attempts per IP address.
pub(crate) struct RateLimiter {
    /// Last connection attempt time per IP.
    ip_last_connect: HashMap<IpAddr, Instant>,
    /// Current connection count per IP.
    ip_connection_count: HashMap<IpAddr, usize>,
}

impl RateLimiter {
    pub(crate) fn new() -> Self {
        Self {
            ip_last_connect: HashMap::new(),
            ip_connection_count: HashMap::new(),
        }
    }

    /// Check if a connection from this IP should be allowed.
    pub(crate) fn check_allowed(&mut self, ip: IpAddr) -> bool {
        let now = Instant::now();

        // Check rate limit - must wait at least CONNECTION_RATE_LIMIT_SECS between attempts
        if let Some(last) = self.ip_last_connect.get(&ip) {
            if now.duration_since(*last) < Duration::from_secs(CONNECTION_RATE_LIMIT_SECS) {
                return false;
            }
        }

        // Check connection count per IP
        let count = self.ip_connection_count.get(&ip).copied().unwrap_or(0);
        if count >= MAX_CONNECTIONS_PER_IP {
            return false;
        }

        self.ip_last_connect.insert(ip, now);
        true
    }

    /// Record that a connection was established from this IP.
    pub(crate) fn record_connection(&mut self, ip: IpAddr) {
        *self.ip_connection_count.entry(ip).or_insert(0) += 1;
    }

    /// Record that a connection was closed from this IP.
    pub(crate) fn record_disconnection(&mut self, ip: IpAddr) {
        if let Some(count) = self.ip_connection_count.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.ip_connection_count.remove(&ip);
            }
        }
    }

    /// Clean up stale entries older than 1 hour.
    #[allow(dead_code)]
    pub(crate) fn cleanup(&mut self) {
        let now = Instant::now();
        let stale_threshold = Duration::from_secs(3600); // 1 hour

        self.ip_last_connect
            .retain(|_, last| now.duration_since(*last) < stale_threshold);
    }
}

/// Subnet prefix type that handles different prefix lengths for IPv4 and IPv6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SubnetPrefix {
    /// IPv4 /16 prefix (2 bytes)
    V4([u8; 2]),
    /// IPv6 /48 prefix (6 bytes) - typical ISP allocation size
    V6([u8; 6]),
}

/// Subnet limiter for eclipse attack protection.
///
/// Limits the number of connections from any single subnet to prevent
/// an attacker from monopolizing connections with IPs from the same subnet.
/// Uses /16 for IPv4 and /48 for IPv6 (typical ISP allocation size).
pub(crate) struct SubnetLimiter {
    /// Connection count per subnet prefix.
    subnet_counts: HashMap<SubnetPrefix, usize>,
}

impl SubnetLimiter {
    pub(crate) fn new() -> Self {
        Self {
            subnet_counts: HashMap::new(),
        }
    }

    /// Extract subnet prefix from IP address.
    ///
    /// For IPv4: returns /16 prefix (first 2 bytes).
    /// For IPv6: returns /48 prefix (first 6 bytes) - typical ISP allocation.
    fn get_subnet_prefix(ip: &IpAddr) -> SubnetPrefix {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                SubnetPrefix::V4([octets[0], octets[1]])
            }
            IpAddr::V6(ipv6) => {
                let octets = ipv6.octets();
                SubnetPrefix::V6([
                    octets[0], octets[1], octets[2], octets[3], octets[4], octets[5],
                ])
            }
        }
    }

    /// Check if we can accept another connection from this subnet.
    pub(crate) fn check_allowed(&self, ip: &IpAddr) -> bool {
        let prefix = Self::get_subnet_prefix(ip);
        let count = self.subnet_counts.get(&prefix).copied().unwrap_or(0);
        count < MAX_PER_SUBNET
    }

    /// Record a new connection from this subnet.
    pub(crate) fn record_connection(&mut self, ip: &IpAddr) {
        let prefix = Self::get_subnet_prefix(ip);
        *self.subnet_counts.entry(prefix).or_insert(0) += 1;
    }

    /// Record a disconnection from this subnet.
    pub(crate) fn record_disconnection(&mut self, ip: &IpAddr) {
        let prefix = Self::get_subnet_prefix(ip);
        if let Some(count) = self.subnet_counts.get_mut(&prefix) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.subnet_counts.remove(&prefix);
            }
        }
    }
}

/// Partition blocklist for testing network partitions.
///
/// Unlike BanList, entries have no expiry time. Uses SocketAddr
/// (not IpAddr) because tests run multiple nodes on localhost with
/// different ports.
pub struct PartitionBlocklist {
    /// Blocked socket addresses.
    blocked_addrs: HashSet<SocketAddr>,
}

impl PartitionBlocklist {
    /// Create a new empty partition blocklist.
    pub fn new() -> Self {
        Self {
            blocked_addrs: HashSet::new(),
        }
    }

    /// Check if a socket address is blocked.
    pub fn is_blocked(&self, addr: &SocketAddr) -> bool {
        self.blocked_addrs.contains(addr)
    }

    /// Block a socket address.
    pub fn block(&mut self, addr: SocketAddr) {
        self.blocked_addrs.insert(addr);
    }

    /// Unblock a socket address.
    pub fn unblock(&mut self, addr: &SocketAddr) {
        self.blocked_addrs.remove(addr);
    }

    /// Clear all blocked addresses.
    pub fn clear(&mut self) {
        self.blocked_addrs.clear();
    }
}

impl Default for PartitionBlocklist {
    fn default() -> Self {
        Self::new()
    }
}
