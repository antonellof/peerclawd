//! WASM sandbox for secure tool execution.
//!
//! This module provides a Wasmtime-based sandbox for running untrusted
//! WASM tools with capability-based permissions and resource limits.

pub mod fuel;
pub mod host;
pub mod sandbox;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;

pub use fuel::{FuelConfig, FuelMeter};
pub use host::{HostCapabilities, HostState};
pub use sandbox::{CompiledModule, WasmSandbox, SandboxConfig};

/// Unique identifier for a WASM tool (BLAKE3 hash of the binary).
pub type ToolHash = String;

/// Result of WASM execution.
#[derive(Debug, Clone)]
pub struct WasmExecutionResult {
    /// Return value
    pub value: serde_json::Value,
    /// Fuel consumed
    pub fuel_consumed: u64,
    /// Execution time
    pub execution_time: Duration,
    /// Memory peak usage in bytes
    pub memory_peak_bytes: u64,
}

/// WASM execution error.
#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    #[error("Compilation failed: {0}")]
    CompilationFailed(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Fuel exhausted after {0} units")]
    FuelExhausted(u64),

    #[error("Memory limit exceeded: {limit}MB")]
    MemoryLimitExceeded { limit: u64 },

    #[error("Timeout after {0:?}")]
    Timeout(Duration),

    #[error("Capability denied: {0}")]
    CapabilityDenied(String),

    #[error("Tool not found: {0}")]
    ToolNotFound(ToolHash),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// WASM tool metadata.
#[derive(Debug, Clone)]
pub struct ToolInfo {
    /// Tool hash (BLAKE3)
    pub hash: ToolHash,
    /// Human-readable name
    pub name: String,
    /// Description
    pub description: String,
    /// Version
    pub version: String,
    /// Required capabilities
    pub required_capabilities: HostCapabilities,
    /// Size in bytes
    pub size_bytes: u64,
}

/// WASM tool registry.
pub struct ToolRegistry {
    /// Tools directory
    tools_dir: PathBuf,
    /// Registered tools
    tools: HashMap<ToolHash, ToolInfo>,
    /// Compiled module cache
    compiled: Arc<RwLock<HashMap<ToolHash, Arc<CompiledModule>>>>,
}

impl ToolRegistry {
    /// Create a new tool registry.
    pub fn new(tools_dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&tools_dir)?;
        Ok(Self {
            tools_dir,
            tools: HashMap::new(),
            compiled: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Register a tool from bytes.
    pub async fn register(&mut self, name: String, wasm_bytes: &[u8]) -> Result<ToolHash, WasmError> {
        // Compute hash
        let hash = blake3::hash(wasm_bytes).to_hex().to_string();

        // Save to disk
        let path = self.tools_dir.join(format!("{}.wasm", &hash[..16]));
        std::fs::write(&path, wasm_bytes)?;

        // Register metadata
        let info = ToolInfo {
            hash: hash.clone(),
            name,
            description: String::new(),
            version: "0.1.0".to_string(),
            required_capabilities: HostCapabilities::default(),
            size_bytes: wasm_bytes.len() as u64,
        };

        self.tools.insert(hash.clone(), info);

        Ok(hash)
    }

    /// Get tool info by hash.
    pub fn get(&self, hash: &str) -> Option<&ToolInfo> {
        self.tools.get(hash)
    }

    /// Get tool path.
    pub fn tool_path(&self, hash: &str) -> PathBuf {
        self.tools_dir.join(format!("{}.wasm", &hash[..16.min(hash.len())]))
    }

    /// List all registered tools.
    pub fn list(&self) -> Vec<&ToolInfo> {
        self.tools.values().collect()
    }

    /// Get or compile a module.
    pub async fn get_compiled(
        &self,
        hash: &str,
        sandbox: &WasmSandbox,
    ) -> Result<Arc<CompiledModule>, WasmError> {
        // Check cache
        {
            let cache = self.compiled.read().await;
            if let Some(module) = cache.get(hash) {
                return Ok(module.clone());
            }
        }

        // Load and compile
        let path = self.tool_path(hash);
        if !path.exists() {
            return Err(WasmError::ToolNotFound(hash.to_string()));
        }

        let wasm_bytes = std::fs::read(&path)?;
        let compiled = sandbox.compile(&wasm_bytes)?;
        let compiled = Arc::new(compiled);

        // Cache
        {
            let mut cache = self.compiled.write().await;
            cache.insert(hash.to_string(), compiled.clone());
        }

        Ok(compiled)
    }
}

/// WASM execution context.
pub struct WasmExecutor {
    /// Sandbox for running WASM
    sandbox: WasmSandbox,
    /// Tool registry
    registry: ToolRegistry,
    /// Configuration
    config: WasmExecutorConfig,
}

impl WasmExecutor {
    /// Create a new WASM executor.
    pub fn new(config: WasmExecutorConfig) -> Result<Self, WasmError> {
        let sandbox_config = SandboxConfig {
            max_memory_bytes: config.max_memory_mb as u64 * 1024 * 1024,
            fuel_limit: config.default_fuel_limit,
            timeout: Duration::from_secs(config.timeout_secs as u64),
        };

        let sandbox = WasmSandbox::new(sandbox_config)?;
        let registry = ToolRegistry::new(config.tools_dir.clone())?;

        Ok(Self {
            sandbox,
            registry,
            config,
        })
    }

    /// Execute a WASM tool.
    pub async fn execute(
        &self,
        tool_hash: &str,
        function: &str,
        params: serde_json::Value,
        capabilities: HostCapabilities,
    ) -> Result<WasmExecutionResult, WasmError> {
        let start = Instant::now();

        // Get compiled module
        let module = self.registry.get_compiled(tool_hash, &self.sandbox).await?;

        // Check capabilities
        if let Some(info) = self.registry.get(tool_hash) {
            if !capabilities.satisfies(&info.required_capabilities) {
                return Err(WasmError::CapabilityDenied(format!(
                    "Tool requires capabilities not granted"
                )));
            }
        }

        // Execute
        let result = self.sandbox.execute(&module, function, params, capabilities)?;

        Ok(WasmExecutionResult {
            value: result.value,
            fuel_consumed: result.fuel_consumed,
            execution_time: start.elapsed(),
            memory_peak_bytes: result.memory_peak_bytes,
        })
    }

    /// Register a new tool.
    pub async fn register_tool(&mut self, name: String, wasm_bytes: &[u8]) -> Result<ToolHash, WasmError> {
        self.registry.register(name, wasm_bytes).await
    }

    /// List available tools.
    pub fn list_tools(&self) -> Vec<&ToolInfo> {
        self.registry.list()
    }
}

/// WASM executor configuration.
#[derive(Debug, Clone)]
pub struct WasmExecutorConfig {
    /// Directory for storing WASM tools
    pub tools_dir: PathBuf,
    /// Maximum memory per execution in MB
    pub max_memory_mb: u32,
    /// Default fuel limit
    pub default_fuel_limit: u64,
    /// Default timeout in seconds
    pub timeout_secs: u32,
}

impl Default for WasmExecutorConfig {
    fn default() -> Self {
        Self {
            tools_dir: crate::bootstrap::base_dir().join("tools"),
            max_memory_mb: 256,
            default_fuel_limit: 100_000_000,
            timeout_secs: 60,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_tool_registry_creation() {
        let dir = tempdir().unwrap();
        let registry = ToolRegistry::new(dir.path().to_path_buf()).unwrap();
        assert!(registry.list().is_empty());
    }
}
