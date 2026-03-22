//! Task types for the distributed execution system.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::job::JobId;

/// Unique identifier for an execution task.
pub type TaskId = String;

/// A task that can be executed locally or remotely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionTask {
    /// LLM inference task
    Inference(InferenceTask),
    /// HTTP fetch task
    WebFetch(WebFetchTask),
    /// Web search task
    WebSearch(WebSearchTask),
    /// WASM tool execution
    WasmExecution(WasmTask),
}

impl ExecutionTask {
    /// Get the task type name for logging/metrics.
    pub fn task_type(&self) -> &'static str {
        match self {
            Self::Inference(_) => "inference",
            Self::WebFetch(_) => "web_fetch",
            Self::WebSearch(_) => "web_search",
            Self::WasmExecution(_) => "wasm",
        }
    }

    /// Estimate the resource requirements for this task.
    pub fn estimate_requirements(&self) -> ResourceRequirements {
        match self {
            Self::Inference(task) => task.estimate_requirements(),
            Self::WebFetch(_) => ResourceRequirements::minimal(),
            Self::WebSearch(_) => ResourceRequirements::minimal(),
            Self::WasmExecution(task) => task.estimate_requirements(),
        }
    }
}

/// LLM inference task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceTask {
    /// Model identifier (e.g., "llama-3.2-8b", "mistral-7b")
    pub model: String,
    /// Chat messages or prompt
    pub messages: Vec<ChatMessage>,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Sampling temperature (0.0 - 2.0)
    pub temperature: f32,
    /// Stop sequences
    pub stop_sequences: Vec<String>,
    /// Stream tokens as they're generated
    pub stream: bool,
}

impl InferenceTask {
    /// Create a new inference task with defaults.
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            messages: vec![ChatMessage::user(prompt)],
            max_tokens: 1024,
            temperature: 0.7,
            stop_sequences: vec![],
            stream: false,
        }
    }

    /// Set maximum tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Set temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }

    /// Enable streaming.
    pub fn with_streaming(mut self) -> Self {
        self.stream = true;
        self
    }

    /// Estimate resource requirements based on model name.
    pub fn estimate_requirements(&self) -> ResourceRequirements {
        // Parse model size from name (e.g., "llama-3.2-8b" -> 8B)
        let params_billions = parse_model_size(&self.model);

        // Rough estimates for GGUF Q4 quantization
        let vram_mb = match params_billions {
            p if p <= 3.0 => 2_000,
            p if p <= 8.0 => 5_000,
            p if p <= 13.0 => 8_000,
            p if p <= 34.0 => 20_000,
            p if p <= 70.0 => 40_000,
            _ => 80_000,
        };

        let ram_mb = vram_mb + 1_000; // Extra for KV cache and overhead

        ResourceRequirements {
            ram_mb,
            vram_mb: Some(vram_mb),
            cpu_cores: 4,
            estimated_duration: Duration::from_secs(30),
        }
    }
}

/// A chat message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    System,
    User,
    Assistant,
}

/// HTTP fetch task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchTask {
    /// URL to fetch
    pub url: String,
    /// HTTP method
    pub method: HttpMethod,
    /// Request headers
    pub headers: Vec<(String, String)>,
    /// Request body (for POST/PUT)
    pub body: Option<Vec<u8>>,
    /// Timeout in seconds
    pub timeout_secs: u32,
}

impl WebFetchTask {
    /// Create a GET request.
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            method: HttpMethod::Get,
            headers: vec![],
            body: None,
            timeout_secs: 30,
        }
    }

    /// Create a POST request.
    pub fn post(url: impl Into<String>, body: impl Into<Vec<u8>>) -> Self {
        Self {
            url: url.into(),
            method: HttpMethod::Post,
            headers: vec![],
            body: Some(body.into()),
            timeout_secs: 30,
        }
    }

    /// Add a header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
}

/// Web search task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchTask {
    /// Search query
    pub query: String,
    /// Maximum results to return
    pub max_results: u32,
    /// Search engine to use
    pub engine: SearchEngine,
}

impl WebSearchTask {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            max_results: 10,
            engine: SearchEngine::Default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    #[default]
    Default,
    Google,
    Bing,
    DuckDuckGo,
}

/// WASM tool execution task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmTask {
    /// Tool identifier (BLAKE3 hash of WASM binary)
    pub tool_hash: String,
    /// Function to call
    pub function: String,
    /// Function parameters as JSON
    pub params: serde_json::Value,
    /// Capabilities requested
    pub capabilities: WasmCapabilities,
    /// Maximum execution time
    pub timeout_secs: u32,
    /// Maximum fuel (execution steps)
    pub max_fuel: u64,
}

impl WasmTask {
    pub fn new(
        tool_hash: impl Into<String>,
        function: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            tool_hash: tool_hash.into(),
            function: function.into(),
            params,
            capabilities: WasmCapabilities::default(),
            timeout_secs: 60,
            max_fuel: 100_000_000,
        }
    }

    pub fn estimate_requirements(&self) -> ResourceRequirements {
        ResourceRequirements {
            ram_mb: 100,
            vram_mb: None,
            cpu_cores: 1,
            estimated_duration: Duration::from_secs(self.timeout_secs as u64),
        }
    }
}

/// Capabilities that can be granted to WASM tools.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WasmCapabilities {
    /// Allow network access
    pub network_access: bool,
    /// Allow filesystem read
    pub filesystem_read: bool,
    /// Allow filesystem write
    pub filesystem_write: bool,
    /// Allowed network hosts (if network_access is true)
    pub allowed_hosts: Vec<String>,
}

/// Resource requirements for task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceRequirements {
    /// Required RAM in MB
    pub ram_mb: u32,
    /// Required VRAM in MB (None = CPU-only)
    pub vram_mb: Option<u32>,
    /// Minimum CPU cores
    pub cpu_cores: u16,
    /// Estimated execution duration
    pub estimated_duration: Duration,
}

impl ResourceRequirements {
    /// Minimal requirements (for lightweight tasks like web fetch).
    pub fn minimal() -> Self {
        Self {
            ram_mb: 50,
            vram_mb: None,
            cpu_cores: 1,
            estimated_duration: Duration::from_secs(10),
        }
    }
}

/// Where the task was executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionLocation {
    /// Executed on this node
    Local,
    /// Executed on a remote peer
    Remote {
        peer_id: String,
        job_id: JobId,
    },
}

/// Result of task execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    /// Task identifier
    pub task_id: TaskId,
    /// Where it was executed
    pub location: ExecutionLocation,
    /// Result data
    pub data: TaskData,
    /// Execution metrics
    pub metrics: TaskMetrics,
    /// Cost in μPCLAW (if remote)
    pub cost: Option<u64>,
}

/// Task-specific result data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskData {
    /// Inference result
    Inference(InferenceResult),
    /// Web fetch result
    WebFetch(WebFetchResult),
    /// Web search result
    WebSearch(WebSearchResult),
    /// WASM execution result
    Wasm(WasmResult),
    /// Task failed
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferenceResult {
    /// Generated text
    pub text: String,
    /// Number of tokens generated
    pub tokens_generated: u32,
    /// Tokens per second
    pub tokens_per_second: f64,
    /// Finish reason
    pub finish_reason: FinishReason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    ContentFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFetchResult {
    /// HTTP status code
    pub status: u16,
    /// Response headers
    pub headers: Vec<(String, String)>,
    /// Response body
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    /// Search results
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmResult {
    /// Return value as JSON
    pub value: serde_json::Value,
    /// Fuel consumed
    pub fuel_consumed: u64,
}

/// Execution metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskMetrics {
    /// Time to first byte/token (ms)
    pub ttfb_ms: u64,
    /// Total execution time (ms)
    pub total_time_ms: u64,
    /// Queue wait time (ms)
    pub queue_time_ms: u64,
}

/// Parse model size from model name.
/// E.g., "llama-3.2-8b" -> 8.0, "mistral-7b-v0.1" -> 7.0
fn parse_model_size(model: &str) -> f32 {
    let lower = model.to_lowercase();

    // Look for patterns like "7b", "8b", "70b", "3.2b"
    for part in lower.split(&['-', '_', ' '][..]) {
        if part.ends_with('b') {
            if let Ok(size) = part.trim_end_matches('b').parse::<f32>() {
                return size;
            }
        }
    }

    // Default to medium size if we can't parse
    7.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_size() {
        assert_eq!(parse_model_size("llama-3.2-8b"), 8.0);
        assert_eq!(parse_model_size("mistral-7b-v0.1"), 7.0);
        assert_eq!(parse_model_size("llama-70b"), 70.0);
        assert_eq!(parse_model_size("phi-3b"), 3.0);
    }

    #[test]
    fn test_inference_task_requirements() {
        let task = InferenceTask::new("llama-3.2-8b", "Hello");
        let req = task.estimate_requirements();
        assert!(req.vram_mb.unwrap() >= 4000);
    }

    #[test]
    fn test_task_type() {
        let task = ExecutionTask::Inference(InferenceTask::new("test", "hello"));
        assert_eq!(task.task_type(), "inference");
    }
}
