//! Runtime coordinator - integrates all subsystems for distributed execution.
//!
//! This module wires up the TaskExecutor, JobManager, InferenceEngine,
//! and P2P network to enable distributed task execution.

use std::sync::Arc;
use std::time::Duration;

use libp2p::PeerId;
use tokio::sync::{mpsc, RwLock};

use crate::config::Config;
use crate::db::Database;
use crate::executor::{
    ExecutorConfig, MonitorConfig, ResourceMonitor, RouterConfig, TaskExecutor,
};
use crate::executor::remote::{JobProvider, RemoteExecutor, RemoteExecutorConfig};
use crate::executor::task::{ExecutionTask, InferenceTask, TaskResult, WebFetchTask, TaskData};
use crate::identity::NodeIdentity;
use crate::inference::{
    InferenceConfig, InferenceEngine, ModelDistributor,
    BatchAggregator, BatchConfig, BatchProcessor, BatchResponse, BatchError, BatchStats,
};
use crate::job::{JobManager, PricingStrategy, network as job_network};
use crate::p2p::{Network, NetworkEvent};
use crate::wallet::{Wallet, WalletConfig};

/// Runtime state containing all integrated subsystems.
pub struct Runtime {
    /// Node identity
    pub identity: Arc<NodeIdentity>,
    /// Database
    pub database: Arc<Database>,
    /// Wallet for token operations
    pub wallet: Arc<Wallet>,
    /// Job manager for marketplace operations
    pub job_manager: Arc<RwLock<JobManager>>,
    /// P2P network
    pub network: Arc<RwLock<Network>>,
    /// Task executor with smart routing
    pub executor: Arc<TaskExecutor>,
    /// Inference engine
    pub inference: Arc<InferenceEngine>,
    /// Model distributor for P2P model sharing
    pub model_distributor: Arc<ModelDistributor>,
    /// Job provider for handling incoming requests
    pub job_provider: Arc<JobProvider>,
    /// Batch aggregator for multi-agent inference
    pub batch_aggregator: Arc<BatchAggregator>,
    /// Local peer ID
    pub local_peer_id: PeerId,
    /// Configuration
    pub config: Config,
}

impl Runtime {
    /// Create a new runtime with all subsystems initialized.
    pub async fn new(
        identity: Arc<NodeIdentity>,
        database: Database,
        config: Config,
    ) -> anyhow::Result<Self> {
        let local_peer_id = *identity.peer_id();
        let database = Arc::new(database);

        // Create wallet
        let wallet = Arc::new(Wallet::new(
            identity.clone(),
            WalletConfig::default(),
            (*database).clone(),
        )?);

        // Credit some initial tokens for testing
        wallet.credit(crate::wallet::to_micro(1000.0), "initial_balance").await?;

        // Create job manager
        let local_peer_id = identity.peer_id().clone();
        let local_peer_id_str = local_peer_id.to_string();
        let job_manager = Arc::new(RwLock::new(JobManager::new(wallet.clone(), local_peer_id_str)));

        // Create network
        let network = Arc::new(RwLock::new(Network::new(&identity, config.p2p.clone())?));

        // Create resource monitor
        let resource_monitor = Arc::new(ResourceMonitor::new(MonitorConfig::default()));

        // Create router config
        let router_config = RouterConfig {
            local_utilization_threshold: config.executor.local_utilization_threshold,
            offload_threshold: config.executor.offload_threshold,
            allow_network_offload: config.executor.allow_network_offload,
            max_concurrent_inference: config.executor.max_concurrent_inference,
            max_concurrent_wasm: config.executor.max_concurrent_wasm,
            local_preference_factor: 1.2, // Prefer local execution by default
        };

        // Create executor config
        let executor_config = crate::executor::ExecutorConfig {
            models_dir: config.inference.models_dir.clone(),
            max_web_response_size: config.executor.max_web_response_size,
            default_web_timeout_secs: config.executor.default_web_timeout_secs,
        };

        // Create task executor
        let executor = TaskExecutor::new(resource_monitor.clone(), router_config, executor_config)
            .with_job_manager(job_manager.clone())
            .with_network(network.clone());
        let executor = Arc::new(executor);

        // Create inference engine
        let inference_config = InferenceConfig {
            models_dir: config.inference.models_dir.clone(),
            max_loaded_models: config.inference.max_loaded_models,
            max_memory_mb: config.inference.max_memory_mb,
            gpu_layers: config.inference.gpu_layers,
            context_size: config.inference.context_size,
            batch_size: config.inference.batch_size,
        };
        let inference = Arc::new(InferenceEngine::new(inference_config)?);

        // Create model distributor
        let model_distributor = Arc::new(ModelDistributor::new(config.inference.models_dir.clone()));

        // Create job provider
        let job_provider = Arc::new(JobProvider::new(
            job_manager.clone(),
            network.clone(),
            local_peer_id.clone(),
        ));

        // Create batch aggregator for multi-agent inference
        let batch_config = BatchConfig {
            batch_window_ms: config.executor.batch_window_ms.unwrap_or(50),
            max_batch_size: config.executor.max_batch_size.unwrap_or(8),
            min_batch_size: config.executor.min_batch_size.unwrap_or(4),
            adaptive: true,
            max_queue_depth: 100,
        };
        let (batch_aggregator, _batch_processor) = BatchAggregator::new(batch_config);
        let batch_aggregator = Arc::new(batch_aggregator);

        Ok(Self {
            identity,
            database,
            wallet,
            job_manager,
            network,
            executor,
            inference,
            model_distributor,
            job_provider,
            batch_aggregator,
            local_peer_id,
            config,
        })
    }

    /// Subscribe to job-related GossipSub topics.
    pub async fn subscribe_to_job_topics(&self) -> anyhow::Result<()> {
        let mut network = self.network.write().await;
        network.subscribe(job_network::topics::JOB_REQUESTS)?;
        network.subscribe(job_network::topics::JOB_BIDS)?;
        network.subscribe(job_network::topics::JOB_STATUS)?;
        tracing::info!("Subscribed to job marketplace topics");
        Ok(())
    }

    /// Set the pricing strategy for this node.
    pub async fn set_pricing(&self, strategy: PricingStrategy) {
        self.job_manager.write().await.set_pricing(strategy).await;
    }

    /// Execute a task (will be routed automatically).
    pub async fn execute_task(&self, task: ExecutionTask) -> Result<TaskResult, crate::executor::ExecutorError> {
        self.executor.execute(task).await
    }

    /// Execute an inference task.
    pub async fn inference(&self, prompt: &str, model: &str, max_tokens: u32) -> Result<TaskResult, crate::executor::ExecutorError> {
        let task = InferenceTask::new(model, prompt).with_max_tokens(max_tokens);
        self.executor.execute(ExecutionTask::Inference(task)).await
    }

    /// Submit inference via batch aggregator (for multi-agent scenarios).
    /// Multiple requests are collected and processed together for efficiency.
    pub async fn inference_batched(
        &self,
        source: &str,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<BatchResponse, BatchError> {
        self.batch_aggregator.submit(
            source.to_string(),
            model.to_string(),
            prompt.to_string(),
            max_tokens,
            temperature,
        ).await
    }

    /// Get batch aggregator statistics.
    pub async fn batch_stats(&self) -> BatchStats {
        self.batch_aggregator.stats().await
    }

    /// Execute inference with streaming - tokens are printed directly to stdout as generated.
    pub async fn inference_streaming_print(
        &self,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<crate::executor::task::InferenceResult, crate::executor::ExecutorError> {
        let task = InferenceTask::new(model, prompt)
            .with_max_tokens(max_tokens)
            .with_temperature(temperature);

        self.executor.execute_inference_streaming_print(task).await
    }

    /// Execute a web fetch task.
    pub async fn web_fetch(&self, url: &str) -> Result<TaskResult, crate::executor::ExecutorError> {
        let task = WebFetchTask::get(url);
        self.executor.execute(ExecutionTask::WebFetch(task)).await
    }

    /// Get resource state.
    pub async fn resource_state(&self) -> crate::executor::ResourceState {
        self.executor.resource_state().await
    }

    /// Get wallet balance (available μPCLAW).
    pub async fn balance(&self) -> u64 {
        self.wallet.balance().await.available
    }

    /// Get connected peers count.
    pub async fn connected_peers_count(&self) -> usize {
        self.network.read().await.connected_peers().len()
    }

    /// Handle a gossip message (job-related).
    pub async fn handle_gossip_message(&self, topic: &str, data: Vec<u8>, source: Option<PeerId>) {
        match topic {
            t if t == job_network::topics::JOB_REQUESTS => {
                if let Ok(msg) = job_network::deserialize_message(&data) {
                    if let job_network::JobMessage::Request(req_msg) = msg {
                        tracing::info!(
                            job_id = %req_msg.request.id,
                            from = %req_msg.requester_peer_id,
                            "Received job request"
                        );
                        if let Err(e) = self.job_provider.handle_request(req_msg).await {
                            tracing::warn!(error = %e, "Failed to handle job request");
                        }
                    }
                }
            }
            t if t == job_network::topics::JOB_BIDS => {
                if let Ok(msg) = job_network::deserialize_message(&data) {
                    if let job_network::JobMessage::Bid(bid_msg) = msg {
                        tracing::debug!(
                            job_id = %bid_msg.bid.job_id,
                            from = %bid_msg.bidder_peer_id,
                            price = bid_msg.bid.price,
                            "Received bid"
                        );
                        if let Err(e) = self.job_manager.write().await.receive_bid(bid_msg.bid).await {
                            tracing::warn!(error = %e, "Failed to process bid");
                        }
                    }
                }
            }
            t if t == job_network::topics::JOB_STATUS => {
                if let Ok(msg) = job_network::deserialize_message(&data) {
                    match msg {
                        job_network::JobMessage::BidAccepted(accept_msg) => {
                            tracing::info!(
                                job_id = %accept_msg.job_id,
                                winner = %accept_msg.winner_peer_id,
                                "Bid accepted"
                            );

                            // Check if we're the winner
                            if accept_msg.winner_peer_id == self.local_peer_id.to_string() {
                                tracing::info!(job_id = %accept_msg.job_id, "We won the bid! Executing job...");

                                // Get the request and execute it
                                let job_id = accept_msg.job_id.clone();
                                if let Some(request) = self.job_provider.get_pending_request(&job_id).await {
                                    self.execute_provider_job(job_id, request).await;
                                }
                            }

                            if let Err(e) = self.job_provider.handle_bid_accepted(accept_msg).await {
                                tracing::warn!(error = %e, "Failed to handle bid acceptance");
                            }
                        }
                        job_network::JobMessage::Result(result_msg) => {
                            tracing::info!(
                                job_id = %result_msg.job_id,
                                provider = %result_msg.provider_peer_id,
                                "Received job result"
                            );
                            // Store result if we're the requester
                            {
                                let job_manager = self.job_manager.write().await;
                                if let Err(e) = job_manager
                                    .submit_result(&result_msg.job_id, result_msg.result).await {
                                    tracing::warn!(error = %e, "Failed to store job result");
                                } else {
                                    // Auto-settle the job (verify and release payment)
                                    if let Err(e) = job_manager.settle_job(&result_msg.job_id, true).await {
                                        tracing::warn!(error = %e, "Failed to settle job");
                                    } else {
                                        tracing::info!(job_id = %result_msg.job_id, "Job settled successfully");
                                    }
                                }
                            }
                        }
                        job_network::JobMessage::StatusUpdate(status_msg) => {
                            tracing::debug!(
                                job_id = %status_msg.job_id,
                                status = ?status_msg.status,
                                "Job status update"
                            );
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}

impl Runtime {
    /// Execute a job as a provider (when our bid was accepted).
    pub async fn execute_provider_job(&self, job_id: crate::job::JobId, request: crate::job::JobRequest) {
        use crate::executor::task::{ExecutionTask, InferenceTask, WebFetchTask, TaskData};
        use crate::job::{JobResult, ActualUsage, ExecutionMetrics};
        use crate::job::network::{JobMessage, JobResultMessage, JobStatusMessage, JobStatusUpdate, serialize_message, topics};

        tracing::info!(job_id = %job_id, "Executing job as provider");

        // Broadcast that we're starting
        let status_msg = JobMessage::StatusUpdate(JobStatusMessage {
            job_id: job_id.clone(),
            status: JobStatusUpdate::Started,
            peer_id: self.local_peer_id.to_string(),
            timestamp: chrono::Utc::now().timestamp() as u64,
        });
        if let Ok(data) = serialize_message(&status_msg) {
            let _ = self.network.write().await.publish(topics::JOB_STATUS, data);
        }

        // Execute based on resource type
        let payload = request.payload.as_ref().map(|p| p.as_slice()).unwrap_or(&[]);
        let result = match &request.resource_type {
            crate::job::ResourceType::Inference { model, tokens } => {
                let prompt_cow = String::from_utf8_lossy(payload);
                let prompt: &str = if prompt_cow.is_empty() { "Hello" } else { prompt_cow.as_ref() };

                let task = InferenceTask::new(model, prompt)
                    .with_max_tokens(*tokens);

                match self.executor.execute(ExecutionTask::Inference(task)).await {
                    Ok(task_result) => {
                        match &task_result.data {
                            TaskData::Inference(r) => {
                                JobResult::new(r.text.as_bytes().to_vec())
                                    .with_usage(ActualUsage {
                                        tokens: Some(r.tokens_generated),
                                        compute_time_ms: Some(task_result.metrics.total_time_ms),
                                        bytes: None,
                                    })
                                    .with_metrics(ExecutionMetrics {
                                        ttfb_ms: task_result.metrics.ttfb_ms,
                                        total_time_ms: task_result.metrics.total_time_ms,
                                        tokens_per_sec: Some(r.tokens_per_second),
                                    })
                            }
                            _ => JobResult::new(b"Unexpected result type".to_vec()),
                        }
                    }
                    Err(e) => JobResult::new(format!("Error: {}", e).into_bytes()),
                }
            }
            crate::job::ResourceType::WebFetch { url_count: _ } => {
                let url = String::from_utf8_lossy(payload);
                let task = WebFetchTask::get(url.as_ref());

                match self.executor.execute(ExecutionTask::WebFetch(task)).await {
                    Ok(task_result) => {
                        match &task_result.data {
                            TaskData::WebFetch(r) => {
                                JobResult::new(r.body.clone())
                                    .with_usage(ActualUsage {
                                        tokens: None,
                                        compute_time_ms: Some(task_result.metrics.total_time_ms),
                                        bytes: Some(r.body.len() as u64),
                                    })
                            }
                            _ => JobResult::new(b"Unexpected result type".to_vec()),
                        }
                    }
                    Err(e) => JobResult::new(format!("Error: {}", e).into_bytes()),
                }
            }
            _ => {
                JobResult::new(b"Unsupported resource type".to_vec())
            }
        };

        tracing::info!(job_id = %job_id, "Job execution complete, sending result");

        // Broadcast result
        let result_msg = JobMessage::Result(JobResultMessage {
            job_id: job_id.clone(),
            result: result.clone(),
            provider_peer_id: self.local_peer_id.to_string(),
            signature: vec![],
        });
        if let Ok(data) = serialize_message(&result_msg) {
            if let Err(e) = self.network.write().await.publish(topics::JOB_STATUS, data) {
                tracing::warn!("Failed to broadcast result: {}", e);
            }
        }

        // Remove from pending
        self.job_provider.remove_pending_request(&job_id).await;

        tracing::info!(job_id = %job_id, "Provider job completed");
    }
}

/// Runtime statistics.
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    pub peer_id: String,
    pub connected_peers: usize,
    pub balance: f64,
    pub active_jobs: usize,
    pub completed_jobs: usize,
    pub resource_state: crate::executor::ResourceState,
}

impl Runtime {
    /// Get runtime statistics.
    pub async fn stats(&self) -> RuntimeStats {
        let active_jobs = self.job_manager.read().await.active_jobs().await.len();
        let completed_jobs = self.job_manager.read().await.completed_jobs(100).await.len();

        RuntimeStats {
            peer_id: self.local_peer_id.to_string(),
            connected_peers: self.connected_peers_count().await,
            balance: crate::wallet::from_micro(self.balance().await),
            active_jobs,
            completed_jobs,
            resource_state: self.resource_state().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_runtime_creation() {
        let dir = tempdir().unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let mut config = Config::default();
        config.database.path = dir.path().join("test.redb");
        config.inference.models_dir = dir.path().join("models");

        let db = Database::open(&config.database.path).unwrap();
        let runtime = Runtime::new(identity, db, config).await;

        assert!(runtime.is_ok());
    }

    #[tokio::test]
    async fn test_runtime_stats() {
        let dir = tempdir().unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let mut config = Config::default();
        config.database.path = dir.path().join("test.redb");
        config.inference.models_dir = dir.path().join("models");

        let db = Database::open(&config.database.path).unwrap();
        let runtime = Runtime::new(identity, db, config).await.unwrap();

        let stats = runtime.stats().await;
        assert_eq!(stats.connected_peers, 0);
        assert!(stats.balance > 0.0); // Should have initial balance
    }
}
