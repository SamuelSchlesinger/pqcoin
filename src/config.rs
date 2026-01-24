//! Configuration management for pqcoin.
//!
//! Configuration is loaded from multiple sources with the following priority (highest first):
//! 1. CLI flags
//! 2. `./pqcoin.toml` (local config)
//! 3. `~/.pqcoin/config.toml` (user config)
//! 4. Built-in defaults

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

use crate::constants::{DEFAULT_PORT, MAX_OUTBOUND, MAX_PEERS};

/// Main configuration structure for the pqcoin node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Network configuration.
    #[serde(default)]
    pub network: NetworkConfig,
    /// Mining configuration.
    #[serde(default)]
    pub mining: MiningConfig,
    /// Logging configuration.
    #[serde(default)]
    pub logging: LoggingConfig,
    /// RPC/API configuration.
    #[serde(default)]
    pub rpc: RpcConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            mining: MiningConfig::default(),
            logging: LoggingConfig::default(),
            rpc: RpcConfig::default(),
        }
    }
}

/// Network configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Port to listen on for P2P connections.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Maximum number of peer connections.
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,
    /// Maximum number of outbound connections.
    #[serde(default = "default_max_outbound")]
    pub max_outbound: usize,
    /// Seed peers to connect to on startup.
    #[serde(default)]
    pub seed_peers: Vec<SocketAddr>,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_max_peers() -> usize {
    MAX_PEERS
}

fn default_max_outbound() -> usize {
    MAX_OUTBOUND
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            max_peers: MAX_PEERS,
            max_outbound: MAX_OUTBOUND,
            seed_peers: Vec::new(),
        }
    }
}

/// Mining configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MiningConfig {
    /// Whether mining is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Miner address to receive block rewards (hex-encoded public key hash).
    #[serde(default)]
    pub address: Option<String>,
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: "trace", "debug", "info", "warn", "error".
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Log format: "text" or "json".
    #[serde(default = "default_log_format")]
    pub format: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "text".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
        }
    }
}

/// RPC/API configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcConfig {
    /// Whether the RPC API is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Port for the RPC API.
    #[serde(default = "default_rpc_port")]
    pub port: u16,
    /// Port for the metrics/health API (RPC port + 1 by default).
    #[serde(default = "default_metrics_port")]
    pub metrics_port: u16,
    /// Address to bind the RPC API to.
    #[serde(default = "default_rpc_bind")]
    pub bind: String,
}

fn default_rpc_port() -> u16 {
    8332
}

fn default_metrics_port() -> u16 {
    9091
}

fn default_rpc_bind() -> String {
    "127.0.0.1".to_string()
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_rpc_port(),
            metrics_port: default_metrics_port(),
            bind: default_rpc_bind(),
        }
    }
}

impl Config {
    /// Load configuration from all sources.
    ///
    /// Configuration is loaded with the following priority (highest first):
    /// 1. Explicit config file (if provided)
    /// 2. `./pqcoin.toml` (local config)
    /// 3. `~/.pqcoin/config.toml` (user config)
    /// 4. Built-in defaults
    pub fn load(config_path: Option<&PathBuf>) -> Result<Self, ConfigError> {
        let mut config = Config::default();

        // Load user config (~/.pqcoin/config.toml)
        if let Some(home_dir) = dirs::home_dir() {
            let user_config_path = home_dir.join(".pqcoin").join("config.toml");
            if user_config_path.exists() {
                let user_config = Self::load_from_file(&user_config_path)?;
                config.merge(user_config);
            }
        }

        // Load local config (./pqcoin.toml)
        let local_config_path = PathBuf::from("pqcoin.toml");
        if local_config_path.exists() {
            let local_config = Self::load_from_file(&local_config_path)?;
            config.merge(local_config);
        }

        // Load explicit config file (highest priority)
        if let Some(path) = config_path {
            let explicit_config = Self::load_from_file(path)?;
            config.merge(explicit_config);
        }

        Ok(config)
    }

    /// Load configuration from a specific file.
    fn load_from_file(path: &PathBuf) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.clone(),
            error: e.to_string(),
        })?;

        toml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: path.clone(),
            error: e.to_string(),
        })
    }

    /// Merge another config into this one.
    /// The other config's non-default values override this config's values.
    fn merge(&mut self, other: Config) {
        // Network
        if other.network.port != DEFAULT_PORT {
            self.network.port = other.network.port;
        }
        if other.network.max_peers != MAX_PEERS {
            self.network.max_peers = other.network.max_peers;
        }
        if other.network.max_outbound != MAX_OUTBOUND {
            self.network.max_outbound = other.network.max_outbound;
        }
        if !other.network.seed_peers.is_empty() {
            self.network.seed_peers.extend(other.network.seed_peers);
        }

        // Mining
        if other.mining.enabled {
            self.mining.enabled = true;
        }
        if other.mining.address.is_some() {
            self.mining.address = other.mining.address;
        }

        // Logging
        if other.logging.level != "info" {
            self.logging.level = other.logging.level;
        }
        if other.logging.format != "text" {
            self.logging.format = other.logging.format;
        }

        // RPC
        if other.rpc.enabled {
            self.rpc.enabled = true;
        }
        if other.rpc.port != 8332 {
            self.rpc.port = other.rpc.port;
        }
        if other.rpc.metrics_port != 9091 {
            self.rpc.metrics_port = other.rpc.metrics_port;
        }
        if other.rpc.bind != "127.0.0.1" {
            self.rpc.bind = other.rpc.bind;
        }
    }

    /// Apply CLI overrides to the configuration.
    pub fn apply_cli_overrides(
        &mut self,
        port: Option<u16>,
        connect: &[SocketAddr],
        mine: bool,
        log_level: Option<&str>,
        log_format: Option<&str>,
        rpc: bool,
        rpc_port: Option<u16>,
        rpc_bind: Option<&str>,
    ) {
        if let Some(p) = port {
            self.network.port = p;
        }
        if !connect.is_empty() {
            self.network.seed_peers.extend_from_slice(connect);
        }
        if mine {
            self.mining.enabled = true;
        }
        if let Some(level) = log_level {
            self.logging.level = level.to_string();
        }
        if let Some(format) = log_format {
            self.logging.format = format.to_string();
        }
        if rpc {
            self.rpc.enabled = true;
        }
        if let Some(p) = rpc_port {
            self.rpc.port = p;
        }
        if let Some(b) = rpc_bind {
            self.rpc.bind = b.to_string();
        }
    }

    /// Generate a sample configuration file.
    pub fn sample() -> String {
        r#"# pqcoin configuration file

[network]
# Port to listen on for P2P connections
port = 8333

# Maximum number of peer connections
max_peers = 125

# Maximum number of outbound connections
max_outbound = 8

# Seed peers to connect to on startup
# seed_peers = ["127.0.0.1:8334", "192.168.1.100:8333"]

[mining]
# Whether mining is enabled
enabled = false

# Miner address to receive block rewards (optional)
# address = "pq1..."

[logging]
# Log level: trace, debug, info, warn, error
level = "info"

# Log format: text or json
format = "text"

[rpc]
# Whether the RPC/API server is enabled
enabled = false

# Port for the JSON-RPC API
port = 8332

# Port for the metrics/health endpoints
metrics_port = 9091

# Address to bind the RPC server to
bind = "127.0.0.1"
"#
        .to_string()
    }
}

/// Configuration errors.
#[derive(Debug, Clone)]
pub enum ConfigError {
    /// I/O error reading config file.
    Io { path: PathBuf, error: String },
    /// Parse error in config file.
    Parse { path: PathBuf, error: String },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io { path, error } => {
                write!(f, "failed to read {}: {}", path.display(), error)
            }
            ConfigError::Parse { path, error } => {
                write!(f, "failed to parse {}: {}", path.display(), error)
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.network.port, DEFAULT_PORT);
        assert_eq!(config.network.max_peers, MAX_PEERS);
        assert!(!config.mining.enabled);
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.logging.format, "text");
        assert!(!config.rpc.enabled);
    }

    #[test]
    fn test_parse_toml() {
        let toml_str = r#"
[network]
port = 9999
seed_peers = ["127.0.0.1:8333"]

[mining]
enabled = true

[logging]
level = "debug"
format = "json"

[rpc]
enabled = true
port = 8888
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.network.port, 9999);
        assert_eq!(config.network.seed_peers.len(), 1);
        assert!(config.mining.enabled);
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.logging.format, "json");
        assert!(config.rpc.enabled);
        assert_eq!(config.rpc.port, 8888);
    }

    #[test]
    fn test_cli_overrides() {
        let mut config = Config::default();
        config.apply_cli_overrides(
            Some(9999),
            &["127.0.0.1:8333".parse().unwrap()],
            true,
            Some("debug"),
            Some("json"),
            true,
            Some(8888),
            Some("0.0.0.0"),
        );

        assert_eq!(config.network.port, 9999);
        assert_eq!(config.network.seed_peers.len(), 1);
        assert!(config.mining.enabled);
        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.logging.format, "json");
        assert!(config.rpc.enabled);
        assert_eq!(config.rpc.port, 8888);
        assert_eq!(config.rpc.bind, "0.0.0.0");
    }
}
