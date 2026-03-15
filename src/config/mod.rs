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

    /// Inference engine configuration
    pub inference: InferenceConfig,

    /// Task executor configuration
    pub executor: ExecutorConfig,

    /// WASM sandbox configuration
    pub wasm: WasmConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            p2p: P2pConfig::default(),
            web: WebConfig::default(),
            resources: ResourcesConfig::default(),
            database: DatabaseConfig::default(),
            agent: AgentConfig::default(),
            inference: InferenceConfig::default(),
            executor: ExecutorConfig::default(),
            wasm: WasmConfig::default(),
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

    /// Save configuration to file.
    pub fn save(&self) -> anyhow::Result<()> {
        let config_path = bootstrap::base_dir().join("config.toml");
        let content = toml::to_string_pretty(self)?;
        std::fs::write(config_path, content)?;
        Ok(())
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

/// Inference engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceConfig {
    /// Directory for model storage
    pub models_dir: PathBuf,
    /// Maximum models to keep loaded
    pub max_loaded_models: usize,
    /// Maximum memory for models in MB
    pub max_memory_mb: u32,
    /// Number of GPU layers to offload (-1 = auto, 0 = CPU only)
    pub gpu_layers: i32,
    /// Context size for inference
    pub context_size: u32,
    /// Batch size for inference
    pub batch_size: u32,
    /// Enable P2P model download
    pub enable_p2p_download: bool,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            models_dir: bootstrap::base_dir().join("models"),
            max_loaded_models: 3,
            max_memory_mb: 16_000, // 16 GB
            gpu_layers: -1,        // Auto
            context_size: 4096,
            batch_size: 512,
            enable_p2p_download: true,
        }
    }
}

/// Task executor configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    /// CPU utilization threshold for local execution (0.0 - 1.0)
    pub local_utilization_threshold: f64,
    /// Utilization threshold above which to offload
    pub offload_threshold: f64,
    /// Allow offloading tasks to network peers
    pub allow_network_offload: bool,
    /// Maximum concurrent inference tasks
    pub max_concurrent_inference: u32,
    /// Maximum concurrent WASM tasks
    pub max_concurrent_wasm: u32,
    /// Maximum web response size in bytes
    pub max_web_response_size: usize,
    /// Default web timeout in seconds
    pub default_web_timeout_secs: u32,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            local_utilization_threshold: 0.8,
            offload_threshold: 0.9,
            allow_network_offload: true,
            max_concurrent_inference: 2,
            max_concurrent_wasm: 10,
            max_web_response_size: 10 * 1024 * 1024, // 10 MB
            default_web_timeout_secs: 30,
        }
    }
}

/// WASM sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmConfig {
    /// Directory for WASM tools
    pub tools_dir: PathBuf,
    /// Maximum memory per execution in MB
    pub max_memory_mb: u32,
    /// Default fuel limit
    pub default_fuel_limit: u64,
    /// Default timeout in seconds
    pub timeout_secs: u32,
}

impl Default for WasmConfig {
    fn default() -> Self {
        Self {
            tools_dir: bootstrap::base_dir().join("tools"),
            max_memory_mb: 256,
            default_fuel_limit: 100_000_000,
            timeout_secs: 60,
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
