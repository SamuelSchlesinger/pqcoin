//! Network defense mechanisms.
//!
//! This module provides protection against various network-level attacks:
//! - `BanList`: Tracks and enforces bans on misbehaving peers
//! - `RateLimiter`: Prevents DoS attacks by limiting connection attempts
//! - `SubnetLimiter`: Protects against eclipse attacks by limiting connections per subnet

use crate::constants::{
    BAN_DURATION_SECS, CONNECTION_RATE_LIMIT_SECS, MAX_CONNECTIONS_PER_IP, MAX_PER_SUBNET,
};
use std::collections::HashMap;
use std::net::IpAddr;
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

/// Subnet limiter for eclipse attack protection.
///
/// Limits the number of connections from any single /16 subnet to prevent
/// an attacker from monopolizing connections with IPs from the same subnet.
pub(crate) struct SubnetLimiter {
    /// Connection count per /16 subnet prefix.
    subnet_counts: HashMap<[u8; 2], usize>,
}

impl SubnetLimiter {
    pub(crate) fn new() -> Self {
        Self {
            subnet_counts: HashMap::new(),
        }
    }

    /// Extract /16 prefix from IP address.
    ///
    /// For IPv4: returns first 2 bytes directly.
    /// For IPv6: returns first 2 bytes of the address (covers /16 equivalent).
    fn get_subnet_prefix(ip: &IpAddr) -> [u8; 2] {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                [octets[0], octets[1]]
            }
            IpAddr::V6(ipv6) => {
                let octets = ipv6.octets();
                [octets[0], octets[1]]
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
