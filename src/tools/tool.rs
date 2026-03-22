//! Tool trait and types.

use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Execution context for tool calls.
#[derive(Debug, Clone)]
pub struct ToolContext {
    /// Current session ID
    pub session_id: String,
    /// Job ID if running within a job
    pub job_id: Option<String>,
    /// Local peer ID
    pub peer_id: String,
    /// Working directory
    pub working_dir: std::path::PathBuf,
    /// Whether running in sandbox mode
    pub sandboxed: bool,
    /// Available secrets (names only, values fetched on demand)
    pub available_secrets: Vec<String>,
}

impl ToolContext {
    /// Create a new context for local execution.
    pub fn local(peer_id: String) -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            job_id: None,
            peer_id,
            working_dir: std::env::current_dir().unwrap_or_default(),
            sandboxed: false,
            available_secrets: vec![],
        }
    }

    /// Create a context for a specific job.
    pub fn for_job(peer_id: String, job_id: String) -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            job_id: Some(job_id),
            peer_id,
            working_dir: std::env::current_dir().unwrap_or_default(),
            sandboxed: false,
            available_secrets: vec![],
        }
    }
}

/// Error type for tool execution.
#[derive(Debug, Error, Clone, Serialize, Deserialize)]
pub enum ToolError {
    #[error("Invalid parameters: {0}")]
    InvalidParameters(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("Timeout after {0} seconds")]
    Timeout(u64),

    #[error("Not authorized: {0}")]
    NotAuthorized(String),

    #[error("Rate limited, retry after {0} seconds")]
    RateLimited(u64),

    #[error("External service error: {0}")]
    ExternalService(String),

    #[error("Capability denied: {0}")]
    CapabilityDenied(String),

    #[error("Tool not found: {0}")]
    NotFound(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Output from tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Success flag
    pub success: bool,
    /// Output data (JSON)
    pub data: serde_json::Value,
    /// Human-readable message
    pub message: Option<String>,
    /// Execution duration
    pub duration_ms: u64,
    /// Any warnings
    pub warnings: Vec<String>,
}

impl ToolOutput {
    /// Create a successful output with data.
    pub fn success(data: serde_json::Value, duration: Duration) -> Self {
        Self {
            success: true,
            data,
            message: None,
            duration_ms: duration.as_millis() as u64,
            warnings: vec![],
        }
    }

    /// Create a successful output with a text message.
    pub fn text(message: impl Into<String>, duration: Duration) -> Self {
        let msg = message.into();
        Self {
            success: true,
            data: serde_json::json!({ "text": &msg }),
            message: Some(msg),
            duration_ms: duration.as_millis() as u64,
            warnings: vec![],
        }
    }

    /// Create a failure output.
    pub fn failure(error: impl Into<String>, duration: Duration) -> Self {
        let err = error.into();
        Self {
            success: false,
            data: serde_json::json!({ "error": &err }),
            message: Some(err),
            duration_ms: duration.as_millis() as u64,
            warnings: vec![],
        }
    }

    /// Add a warning to the output.
    pub fn with_warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }
}

/// How much approval a tool invocation requires.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalRequirement {
    /// No approval needed.
    Never,
    /// Needs approval, but auto-approve can bypass.
    #[default]
    UnlessAutoApproved,
    /// Always needs explicit approval.
    Always,
}

/// Where a tool should execute.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolDomain {
    /// Safe to run anywhere (pure functions, queries).
    #[default]
    Any,
    /// Must run locally (filesystem, shell).
    Local,
    /// Can run on remote peers (inference, computation).
    Remote,
}

/// The Tool trait defines the interface for all tools.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (unique identifier).
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str;

    /// JSON Schema for parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Execute the tool with given parameters.
    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;

    /// Approval requirement for this tool.
    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    /// Execution domain for this tool.
    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    /// Whether output should be sanitized (external content).
    fn requires_sanitization(&self) -> bool {
        true
    }

    /// Rate limit configuration (requests per minute).
    fn rate_limit(&self) -> Option<u32> {
        None
    }
}

/// Helper to extract a required string parameter.
pub fn require_str<'a>(params: &'a serde_json::Value, key: &str) -> Result<&'a str, ToolError> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidParameters(format!("Missing required parameter: {}", key)))
}

/// Helper to extract an optional string parameter.
pub fn optional_str<'a>(params: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(|v| v.as_str())
}

/// Helper to extract a required integer parameter.
pub fn require_i64(params: &serde_json::Value, key: &str) -> Result<i64, ToolError> {
    params
        .get(key)
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ToolError::InvalidParameters(format!("Missing required integer: {}", key)))
}

/// Helper to extract an optional integer parameter with default.
pub fn optional_i64(params: &serde_json::Value, key: &str, default: i64) -> i64 {
    params.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

/// Helper to extract a required boolean parameter.
pub fn require_bool(params: &serde_json::Value, key: &str) -> Result<bool, ToolError> {
    params
        .get(key)
        .and_then(|v| v.as_bool())
        .ok_or_else(|| ToolError::InvalidParameters(format!("Missing required boolean: {}", key)))
}

/// Helper to extract an optional boolean parameter with default.
pub fn optional_bool(params: &serde_json::Value, key: &str, default: bool) -> bool {
    params.get(key).and_then(|v| v.as_bool()).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_output_success() {
        let output = ToolOutput::success(
            serde_json::json!({"result": 42}),
            Duration::from_millis(100),
        );
        assert!(output.success);
        assert_eq!(output.data["result"], 42);
    }

    #[test]
    fn test_tool_output_text() {
        let output = ToolOutput::text("Hello, world!", Duration::from_millis(50));
        assert!(output.success);
        assert_eq!(output.message, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_tool_output_failure() {
        let output = ToolOutput::failure("Something went wrong", Duration::from_millis(10));
        assert!(!output.success);
        assert!(output.message.unwrap().contains("wrong"));
    }

    #[test]
    fn test_require_str() {
        let params = serde_json::json!({"name": "test", "count": 5});
        assert_eq!(require_str(&params, "name").unwrap(), "test");
        assert!(require_str(&params, "missing").is_err());
    }
}
