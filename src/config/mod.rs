//! Configuration management with layered resolution.
//!
//! Priority: defaults -> config file -> env vars -> CLI flags

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;

use crate::bootstrap;

/// Root configuration for PeerClaw'd.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// P2P networking configuration
    pub p2p: P2pConfig,

    /// Web dashboard configuration
    pub web: WebConfig,

    /// Resource advertisement configuration
    pub resources: ResourcesConfig,

    /// Database configuration
    pub database: DatabaseConfig,

    /// Agent configuration
    pub agent: AgentConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            p2p: P2pConfig::default(),
            web: WebConfig::default(),
            resources: ResourcesConfig::default(),
            database: DatabaseConfig::default(),
            agent: AgentConfig::default(),
        }
    }
}

impl Config {
    /// Load configuration with layered resolution.
    pub fn load() -> anyhow::Result<Self> {
        let mut config = Self::default();

        // Try to load from config file
        let config_path = bootstrap::base_dir().join("config.toml");
        if config_path.exists() {
            let content = std::fs::read_to_string(&config_path)?;
            config = toml::from_str(&content)?;
        }

        // Apply environment variable overrides
        config.apply_env_overrides();

        Ok(config)
    }

    /// Apply environment variable overrides.
    fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("PEERCLAWD_WEB_ENABLED") {
            self.web.enabled = val.parse().unwrap_or(self.web.enabled);
        }

        if let Ok(val) = std::env::var("PEERCLAWD_WEB_ADDR") {
            if let Ok(addr) = val.parse() {
                self.web.listen_addr = addr;
            }
        }

        if let Ok(val) = std::env::var("PEERCLAWD_BOOTSTRAP") {
            self.p2p.bootstrap_peers = val.split(',').map(String::from).collect();
        }
    }
}

/// P2P networking configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2pConfig {
    /// Listen addresses for P2P connections
    pub listen_addresses: Vec<String>,

    /// Bootstrap peers to connect to
    pub bootstrap_peers: Vec<String>,

    /// Enable mDNS for local discovery
    pub mdns_enabled: bool,

    /// Enable Kademlia DHT
    pub kademlia_enabled: bool,

    /// Resource advertisement interval in seconds
    pub advertise_interval_secs: u64,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            listen_addresses: vec![
                "/ip4/0.0.0.0/tcp/0".to_string(),
            ],
            bootstrap_peers: vec![],
            mdns_enabled: true,
            kademlia_enabled: true,
            advertise_interval_secs: 300, // 5 minutes
        }
    }
}

/// Web dashboard configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    /// Enable web dashboard
    pub enabled: bool,

    /// Listen address for web server
    pub listen_addr: SocketAddr,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            listen_addr: "127.0.0.1:8080".parse().unwrap(),
        }
    }
}

/// Resource advertisement configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesConfig {
    /// Advertise GPU resources
    pub advertise_gpu: bool,

    /// CPU cores to advertise (None = auto-detect)
    pub cpu_cores: Option<u16>,

    /// Storage to advertise in bytes (None = auto-detect)
    pub storage_bytes: Option<u64>,

    /// RAM to advertise in MB (None = auto-detect)
    pub ram_mb: Option<u32>,
}

impl Default for ResourcesConfig {
    fn default() -> Self {
        Self {
            advertise_gpu: false,
            cpu_cores: None,
            storage_bytes: None,
            ram_mb: None,
        }
    }
}

/// Database configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Path to the database file
    pub path: PathBuf,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: bootstrap::database_path(),
        }
    }
}

/// Agent configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum concurrent agents
    pub max_agents: usize,

    /// Default model for agents
    pub default_model: String,

    /// WASM tool timeout in seconds
    pub tool_timeout_secs: u64,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_agents: 10,
            default_model: "llama-3.2-3b".to_string(),
            tool_timeout_secs: 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(!config.web.enabled);
        assert!(config.p2p.mdns_enabled);
        assert!(config.p2p.kademlia_enabled);
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(config.p2p.mdns_enabled, parsed.p2p.mdns_enabled);
    }
}
