//! Tool system for AI agent capabilities.
//!
//! This module provides extensible tool infrastructure for the AI agent:
//! - Built-in tools (shell, http, file, memory, time, json)
//! - WASM tools (sandboxed, capability-based)
//! - MCP protocol support (external tool servers)
//!
//! Tools can execute locally or be distributed across the P2P network.

pub mod builtin;
pub mod registry;
pub mod tool;

pub use registry::{ToolRegistry, ToolInfo};
pub use tool::{Tool, ToolContext, ToolError, ToolOutput};

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Tool execution location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolLocation {
    /// Execute locally on this node.
    Local,
    /// Execute on a remote peer.
    Remote,
    /// Execute on best available peer (local or remote).
    Auto,
}

/// Tool execution result with P2P metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// Tool output
    pub output: ToolOutput,
    /// Which peer executed the tool
    pub executed_by: String,
    /// Whether it was local or remote execution
    pub location: ToolLocation,
    /// Execution time
    pub execution_time_ms: u64,
    /// Tokens consumed (for billing)
    pub tokens_consumed: Option<u64>,
}

/// Tool capabilities for WASM sandbox.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolCapabilities {
    /// Can make HTTP requests
    pub http_access: bool,
    /// Allowed HTTP hosts
    pub http_allowlist: Vec<String>,
    /// Can read files
    pub file_read: bool,
    /// Can write files
    pub file_write: bool,
    /// Can execute shell commands
    pub shell_access: bool,
    /// Can access secrets
    pub secrets_access: bool,
    /// Can invoke other tools
    pub tool_invocation: bool,
    /// Maximum execution time
    pub max_execution_time: Option<Duration>,
    /// Maximum memory usage in MB
    pub max_memory_mb: Option<u32>,
}

impl ToolCapabilities {
    /// Full capabilities (trusted tools).
    pub fn full() -> Self {
        Self {
            http_access: true,
            http_allowlist: vec!["*".to_string()],
            file_read: true,
            file_write: true,
            shell_access: true,
            secrets_access: true,
            tool_invocation: true,
            max_execution_time: None,
            max_memory_mb: None,
        }
    }

    /// Read-only capabilities (safe for untrusted content).
    pub fn read_only() -> Self {
        Self {
            http_access: false,
            http_allowlist: vec![],
            file_read: true,
            file_write: false,
            shell_access: false,
            secrets_access: false,
            tool_invocation: false,
            max_execution_time: Some(Duration::from_secs(30)),
            max_memory_mb: Some(128),
        }
    }

    /// Network-only capabilities.
    pub fn network_only(hosts: Vec<String>) -> Self {
        Self {
            http_access: true,
            http_allowlist: hosts,
            file_read: false,
            file_write: false,
            shell_access: false,
            secrets_access: false,
            tool_invocation: false,
            max_execution_time: Some(Duration::from_secs(60)),
            max_memory_mb: Some(64),
        }
    }
}

/// Tool authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuth {
    /// Secret name in the secrets store
    pub secret_name: String,
    /// Display name for UI
    pub display_name: String,
    /// Setup instructions
    pub instructions: Option<String>,
    /// URL to get credentials
    pub setup_url: Option<String>,
    /// Environment variable fallback
    pub env_var: Option<String>,
    /// OAuth configuration (if supported)
    pub oauth: Option<OAuthConfig>,
}

/// OAuth configuration for tool authentication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub authorization_url: String,
    pub token_url: String,
    pub client_id_env: String,
    pub client_secret_env: String,
    pub scopes: Vec<String>,
    pub use_pkce: bool,
}

/// Tool manifest (from capabilities.json).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    /// Tool name
    pub name: String,
    /// Version
    pub version: String,
    /// Description
    pub description: String,
    /// Required capabilities
    pub capabilities: ToolCapabilities,
    /// Authentication requirements
    pub auth: Option<ToolAuth>,
    /// Tool parameters schema (JSON Schema)
    pub parameters_schema: serde_json::Value,
}
