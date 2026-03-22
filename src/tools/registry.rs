//! Tool registry for discovery and execution.
//!
//! The registry manages both local and network-distributed tools:
//! - Local builtin tools (shell, file, etc.)
//! - WASM tools (sandboxed, capability-based)
//! - Remote peer tools (discovered via P2P)
//! - MCP protocol servers

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;

use super::builtin;
use super::tool::{Tool, ToolContext, ToolError, ToolDomain};
use super::{ToolCapabilities, ToolLocation, ToolResult};

/// Tool availability on the network.
#[derive(Debug, Clone)]
pub struct NetworkToolInfo {
    /// Tool name
    pub name: String,
    /// Peer ID offering this tool
    pub peer_id: String,
    /// Price per invocation (in micro-tokens)
    pub price_per_call: u64,
    /// Average latency in ms
    pub avg_latency_ms: u32,
    /// Reliability score (0-100)
    pub reliability: u8,
    /// Last seen timestamp
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

/// Tool registry manages all available tools.
pub struct ToolRegistry {
    /// Local peer ID
    local_peer_id: String,
    /// Builtin tools
    builtin_tools: HashMap<String, Arc<dyn Tool>>,
    /// WASM tools
    wasm_tools: RwLock<HashMap<String, WasmToolEntry>>,
    /// Network-discovered tools from other peers
    network_tools: RwLock<HashMap<String, Vec<NetworkToolInfo>>>,
    /// Tool execution statistics
    stats: RwLock<HashMap<String, ToolStats>>,
}

/// WASM tool entry.
#[allow(dead_code)]
struct WasmToolEntry {
    name: String,
    description: String,
    parameters_schema: serde_json::Value,
    capabilities: ToolCapabilities,
    wasm_path: std::path::PathBuf,
}

/// Tool execution statistics.
#[derive(Debug, Clone, Default)]
pub struct ToolStats {
    pub total_calls: u64,
    pub successful_calls: u64,
    pub failed_calls: u64,
    pub total_time_ms: u64,
    pub tokens_earned: u64,
    pub tokens_spent: u64,
}

impl ToolRegistry {
    /// Create a new tool registry with builtin tools.
    pub fn new(local_peer_id: String) -> Self {
        let mut registry = Self {
            local_peer_id,
            builtin_tools: HashMap::new(),
            wasm_tools: RwLock::new(HashMap::new()),
            network_tools: RwLock::new(HashMap::new()),
            stats: RwLock::new(HashMap::new()),
        };
        registry.register_builtin_tools();
        registry
    }

    /// Register all builtin tools.
    fn register_builtin_tools(&mut self) {
        // Core tools
        self.register_builtin(Arc::new(builtin::EchoTool));
        self.register_builtin(Arc::new(builtin::TimeTool));
        self.register_builtin(Arc::new(builtin::JsonTool));

        // HTTP tools
        self.register_builtin(Arc::new(builtin::HttpTool::new()));
        self.register_builtin(Arc::new(builtin::WebFetchTool::new()));

        // File tools
        self.register_builtin(Arc::new(builtin::FileReadTool));
        self.register_builtin(Arc::new(builtin::FileWriteTool));
        self.register_builtin(Arc::new(builtin::FileListTool));

        // Shell tool
        self.register_builtin(Arc::new(builtin::ShellTool::new()));

        // P2P-native tools
        self.register_builtin(Arc::new(builtin::MemorySearchTool::new()));
        self.register_builtin(Arc::new(builtin::MemoryWriteTool::new()));
        self.register_builtin(Arc::new(builtin::JobSubmitTool::new()));
        self.register_builtin(Arc::new(builtin::JobStatusTool::new()));
        self.register_builtin(Arc::new(builtin::PeerDiscoveryTool::new()));
        self.register_builtin(Arc::new(builtin::WalletBalanceTool::new()));
    }

    /// Register a builtin tool.
    fn register_builtin(&mut self, tool: Arc<dyn Tool>) {
        self.builtin_tools.insert(tool.name().to_string(), tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.builtin_tools.get(name).cloned()
    }

    /// List all available tools (local + network).
    pub async fn list_tools(&self) -> Vec<ToolInfo> {
        let mut tools = Vec::new();

        // Builtin tools
        for (name, tool) in &self.builtin_tools {
            tools.push(ToolInfo {
                name: name.clone(),
                description: tool.description().to_string(),
                domain: tool.domain(),
                location: ToolLocation::Local,
                price: 0, // Builtin tools are free
                peer_id: Some(self.local_peer_id.clone()),
            });
        }

        // WASM tools
        let wasm = self.wasm_tools.read().await;
        for (name, entry) in wasm.iter() {
            tools.push(ToolInfo {
                name: name.clone(),
                description: entry.description.clone(),
                domain: ToolDomain::Local, // WASM runs locally
                location: ToolLocation::Local,
                price: 0,
                peer_id: Some(self.local_peer_id.clone()),
            });
        }

        // Network tools
        let network = self.network_tools.read().await;
        for (name, providers) in network.iter() {
            if let Some(best) = providers.first() {
                tools.push(ToolInfo {
                    name: name.clone(),
                    description: format!("Remote tool from {}", best.peer_id),
                    domain: ToolDomain::Remote,
                    location: ToolLocation::Remote,
                    price: best.price_per_call,
                    peer_id: Some(best.peer_id.clone()),
                });
            }
        }

        tools
    }

    /// Execute a tool locally.
    pub async fn execute_local(
        &self,
        name: &str,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let start = Instant::now();

        let tool = self.get(name)
            .ok_or_else(|| ToolError::NotFound(name.to_string()))?;

        let output = tool.execute(params, ctx).await?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            let entry = stats.entry(name.to_string()).or_default();
            entry.total_calls += 1;
            if output.success {
                entry.successful_calls += 1;
            } else {
                entry.failed_calls += 1;
            }
            entry.total_time_ms += start.elapsed().as_millis() as u64;
        }

        Ok(ToolResult {
            output,
            executed_by: self.local_peer_id.clone(),
            location: ToolLocation::Local,
            execution_time_ms: start.elapsed().as_millis() as u64,
            tokens_consumed: None,
        })
    }

    /// Register a network tool from peer announcement.
    pub async fn register_network_tool(&self, info: NetworkToolInfo) {
        let mut network = self.network_tools.write().await;
        let providers = network.entry(info.name.clone()).or_insert_with(Vec::new);

        // Update or add provider
        if let Some(existing) = providers.iter_mut().find(|p| p.peer_id == info.peer_id) {
            *existing = info;
        } else {
            providers.push(info);
        }

        // Sort by price and reliability
        providers.sort_by(|a, b| {
            let a_score = a.price_per_call as f64 / (a.reliability as f64 + 1.0);
            let b_score = b.price_per_call as f64 / (b.reliability as f64 + 1.0);
            a_score.partial_cmp(&b_score).unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Get best provider for a network tool.
    pub async fn best_provider(&self, name: &str) -> Option<NetworkToolInfo> {
        let network = self.network_tools.read().await;
        network.get(name).and_then(|providers| providers.first().cloned())
    }

    /// Get tool execution statistics.
    pub async fn get_stats(&self, name: &str) -> Option<ToolStats> {
        self.stats.read().await.get(name).cloned()
    }

    /// Get all tool statistics.
    pub async fn all_stats(&self) -> HashMap<String, ToolStats> {
        self.stats.read().await.clone()
    }

    /// Count total available tools.
    pub async fn count(&self) -> usize {
        let wasm_count = self.wasm_tools.read().await.len();
        let network_count = self.network_tools.read().await.len();
        self.builtin_tools.len() + wasm_count + network_count
    }

    /// Clear stale network tools (older than given duration).
    pub async fn cleanup_stale_tools(&self, max_age: chrono::Duration) {
        let mut network = self.network_tools.write().await;
        let now = chrono::Utc::now();

        for providers in network.values_mut() {
            providers.retain(|p| now - p.last_seen < max_age);
        }

        // Remove empty entries
        network.retain(|_, providers| !providers.is_empty());
    }
}

/// Tool information for listing.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub domain: ToolDomain,
    pub location: ToolLocation,
    pub price: u64,
    pub peer_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ToolRegistry::new("test-peer".to_string());
        assert!(registry.get("echo").is_some());
        assert!(registry.get("time").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn test_list_tools() {
        let registry = ToolRegistry::new("test-peer".to_string());
        let tools = registry.list_tools().await;
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|t| t.name == "echo"));
    }

    #[tokio::test]
    async fn test_execute_echo() {
        let registry = ToolRegistry::new("test-peer".to_string());
        let ctx = ToolContext::local("test-peer".to_string());

        let result = registry.execute_local(
            "echo",
            serde_json::json!({"message": "hello"}),
            &ctx,
        ).await.unwrap();

        assert!(result.output.success);
        assert_eq!(result.location, ToolLocation::Local);
    }
}
