//! Unified task execution system with automatic routing.
//!
//! The executor module provides a unified interface for running tasks
//! (inference, web access, WASM execution) either locally or distributed
//! across the P2P network.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────┐
//! │TaskExecutor │
//! └──────┬──────┘
//!        │
//!        ▼
//! ┌─────────────┐     ┌─────────────────┐
//! │ TaskRouter  │◄────│ ResourceMonitor │
//! └──────┬──────┘     └─────────────────┘
//!        │
//!   ┌────┴────┐
//!   │         │
//!   ▼         ▼
//! Local    Network
//! ```

pub mod remote;
pub mod resource;
pub mod router;
pub mod task;

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;
use uuid::Uuid;

pub use resource::{MonitorConfig, ResourceMonitor, ResourceState, TaskType};
pub use router::{PeerFilter, RouterConfig, RoutingDecision, TaskRouter};
pub use task::*;

use crate::job::JobManager;
use crate::p2p::Network;

/// Unified task executor with automatic local/network routing.
pub struct TaskExecutor {
    router: TaskRouter,
    resource_monitor: Arc<ResourceMonitor>,
    job_manager: Option<Arc<RwLock<JobManager>>>,
    network: Option<Arc<RwLock<Network>>>,
    config: ExecutorConfig,
}

impl TaskExecutor {
    /// Create a new task executor.
    pub fn new(
        resource_monitor: Arc<ResourceMonitor>,
        router_config: RouterConfig,
        config: ExecutorConfig,
    ) -> Self {
        let router = TaskRouter::new(resource_monitor.clone(), router_config);

        Self {
            router,
            resource_monitor,
            job_manager: None,
            network: None,
            config,
        }
    }

    /// Set the job manager for remote execution.
    pub fn with_job_manager(mut self, job_manager: Arc<RwLock<JobManager>>) -> Self {
        self.job_manager = Some(job_manager);
        self
    }

    /// Set the network for P2P operations.
    pub fn with_network(mut self, network: Arc<RwLock<Network>>) -> Self {
        self.network = Some(network);
        self
    }

    /// Execute a task, routing automatically based on resources.
    pub async fn execute(&self, task: ExecutionTask) -> Result<TaskResult, ExecutorError> {
        let task_id = Uuid::new_v4().to_string();
        let start = Instant::now();

        // Get routing decision
        let decision = self.router.route(&task).await;

        tracing::debug!(
            task_id = %task_id,
            task_type = %task.task_type(),
            decision = ?decision,
            "Routing task"
        );

        match decision {
            RoutingDecision::ExecuteLocally => {
                self.execute_local(task_id, task, start).await
            }
            RoutingDecision::OffloadToNetwork { requirements } => {
                self.execute_remote(task_id, task, requirements, start)
                    .await
            }
            RoutingDecision::InsufficientResources { reason } => {
                Err(ExecutorError::InsufficientResources(reason))
            }
        }
    }

    /// Force local execution (skip routing).
    pub async fn execute_local(
        &self,
        task_id: TaskId,
        task: ExecutionTask,
        start: Instant,
    ) -> Result<TaskResult, ExecutorError> {
        let task_type = match &task {
            ExecutionTask::Inference(_) => TaskType::Inference,
            ExecutionTask::WasmExecution(_) => TaskType::Wasm,
            ExecutionTask::WebFetch(_) | ExecutionTask::WebSearch(_) => TaskType::Web,
        };

        // Track active task
        self.resource_monitor.task_started(task_type).await;

        let result = match task {
            ExecutionTask::Inference(inference_task) => {
                self.execute_inference_local(inference_task).await
            }
            ExecutionTask::WebFetch(fetch_task) => {
                self.execute_web_fetch_local(fetch_task).await
            }
            ExecutionTask::WebSearch(search_task) => {
                self.execute_web_search_local(search_task).await
            }
            ExecutionTask::WasmExecution(wasm_task) => {
                self.execute_wasm_local(wasm_task).await
            }
        };

        // Task completed
        self.resource_monitor.task_completed(task_type).await;

        let total_time_ms = start.elapsed().as_millis() as u64;

        match result {
            Ok(data) => Ok(TaskResult {
                task_id,
                location: ExecutionLocation::Local,
                data,
                metrics: TaskMetrics {
                    ttfb_ms: 0, // TODO: Track actual TTFB
                    total_time_ms,
                    queue_time_ms: 0,
                },
                cost: None,
            }),
            Err(e) => Ok(TaskResult {
                task_id,
                location: ExecutionLocation::Local,
                data: TaskData::Error(e.to_string()),
                metrics: TaskMetrics {
                    ttfb_ms: 0,
                    total_time_ms,
                    queue_time_ms: 0,
                },
                cost: None,
            }),
        }
    }

    /// Execute via network (offload to peer).
    async fn execute_remote(
        &self,
        task_id: TaskId,
        task: ExecutionTask,
        _peer_filter: PeerFilter,
        start: Instant,
    ) -> Result<TaskResult, ExecutorError> {
        let _job_manager = self
            .job_manager
            .as_ref()
            .ok_or(ExecutorError::NetworkUnavailable)?;

        // TODO: Implement full remote execution flow:
        // 1. Serialize task to JobRequest payload
        // 2. Broadcast request via JobManager
        // 3. Collect bids from network peers
        // 4. Select best bid
        // 5. Create escrow
        // 6. Wait for result
        // 7. Verify and settle

        let total_time_ms = start.elapsed().as_millis() as u64;

        // For now, return a placeholder indicating remote execution isn't fully implemented
        tracing::warn!(
            task_id = %task_id,
            task_type = %task.task_type(),
            "Remote execution not yet implemented, falling back to local"
        );

        // Fallback to local for now
        self.execute_local(task_id, task, start).await
    }

    /// Execute inference locally.
    async fn execute_inference_local(
        &self,
        task: InferenceTask,
    ) -> Result<TaskData, ExecutorError> {
        // TODO: Implement via inference module
        // For now, return a placeholder
        tracing::info!(
            model = %task.model,
            max_tokens = task.max_tokens,
            "Would execute inference locally"
        );

        // Placeholder response
        Ok(TaskData::Inference(InferenceResult {
            text: format!(
                "[Inference placeholder - model: {}, tokens requested: {}]",
                task.model, task.max_tokens
            ),
            tokens_generated: 0,
            tokens_per_second: 0.0,
            finish_reason: FinishReason::Stop,
        }))
    }

    /// Execute web fetch locally.
    async fn execute_web_fetch_local(
        &self,
        task: WebFetchTask,
    ) -> Result<TaskData, ExecutorError> {
        use reqwest::Client;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(task.timeout_secs as u64))
            .build()
            .map_err(|e| ExecutorError::WebError(e.to_string()))?;

        let mut request = match task.method {
            HttpMethod::Get => client.get(&task.url),
            HttpMethod::Post => client.post(&task.url),
            HttpMethod::Put => client.put(&task.url),
            HttpMethod::Delete => client.delete(&task.url),
            HttpMethod::Patch => client.patch(&task.url),
            HttpMethod::Head => client.head(&task.url),
        };

        // Add headers
        for (key, value) in &task.headers {
            request = request.header(key.as_str(), value.as_str());
        }

        // Add body if present
        if let Some(body) = task.body {
            request = request.body(body);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ExecutorError::WebError(e.to_string()))?;

        let status = response.status().as_u16();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let body = response
            .bytes()
            .await
            .map_err(|e| ExecutorError::WebError(e.to_string()))?
            .to_vec();

        Ok(TaskData::WebFetch(WebFetchResult {
            status,
            headers,
            body,
        }))
    }

    /// Execute web search locally.
    async fn execute_web_search_local(
        &self,
        task: WebSearchTask,
    ) -> Result<TaskData, ExecutorError> {
        // TODO: Implement actual web search
        // For now, return placeholder
        tracing::info!(
            query = %task.query,
            engine = ?task.engine,
            "Would execute web search"
        );

        Ok(TaskData::WebSearch(WebSearchResult { results: vec![] }))
    }

    /// Execute WASM tool locally.
    async fn execute_wasm_local(&self, task: WasmTask) -> Result<TaskData, ExecutorError> {
        // TODO: Implement via wasm sandbox module
        tracing::info!(
            tool_hash = %task.tool_hash,
            function = %task.function,
            "Would execute WASM tool"
        );

        Ok(TaskData::Wasm(WasmResult {
            value: serde_json::json!({"status": "not_implemented"}),
            fuel_consumed: 0,
        }))
    }

    /// Get current resource state.
    pub async fn resource_state(&self) -> ResourceState {
        self.resource_monitor.current_state().await
    }

    /// Get the resource monitor.
    pub fn resource_monitor(&self) -> Arc<ResourceMonitor> {
        self.resource_monitor.clone()
    }

    /// Check if a model is loaded locally.
    pub async fn has_model(&self, model_id: &str) -> bool {
        let state = self.resource_monitor.current_state().await;
        state.has_model(model_id)
    }
}

/// Executor configuration.
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Models directory
    pub models_dir: std::path::PathBuf,
    /// Maximum response size for web fetch (bytes)
    pub max_web_response_size: usize,
    /// Default timeout for web requests (seconds)
    pub default_web_timeout_secs: u32,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            models_dir: crate::bootstrap::base_dir().join("models"),
            max_web_response_size: 10 * 1024 * 1024, // 10 MB
            default_web_timeout_secs: 30,
        }
    }
}

/// Errors from task execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecutorError {
    #[error("Insufficient resources: {0}")]
    InsufficientResources(String),

    #[error("Network unavailable for remote execution")]
    NetworkUnavailable,

    #[error("Web error: {0}")]
    WebError(String),

    #[error("Inference error: {0}")]
    InferenceError(String),

    #[error("WASM error: {0}")]
    WasmError(String),

    #[error("Timeout")]
    Timeout,

    #[error("Task cancelled")]
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_executor() -> TaskExecutor {
        let monitor = Arc::new(ResourceMonitor::with_defaults());
        // Disable network offload so tests run locally
        let mut router_config = RouterConfig::default();
        router_config.allow_network_offload = false;
        TaskExecutor::new(monitor, router_config, ExecutorConfig::default())
    }

    #[tokio::test]
    async fn test_execute_web_fetch_local() {
        let executor = create_executor();

        // Execute locally directly
        let task = WebFetchTask::get("https://httpbin.org/get");
        let result = executor
            .execute_web_fetch_local(task)
            .await
            .expect("Web fetch should succeed");

        match result {
            TaskData::WebFetch(fetch_result) => {
                assert_eq!(fetch_result.status, 200);
            }
            _ => panic!("Expected WebFetch result"),
        }
    }

    #[tokio::test]
    async fn test_execute_inference_placeholder() {
        let executor = create_executor();

        // Execute locally directly
        let task = InferenceTask::new("llama-7b", "Hello, world!");
        let result = executor
            .execute_inference_local(task)
            .await
            .expect("Inference should succeed");

        match result {
            TaskData::Inference(_) => {}
            _ => panic!("Expected Inference result"),
        }
    }

    #[tokio::test]
    async fn test_resource_state() {
        let monitor = Arc::new(ResourceMonitor::with_defaults());
        let state = monitor.current_state().await;

        // Default state should have some defaults
        assert_eq!(state.active_inference_tasks, 0);
    }
}
