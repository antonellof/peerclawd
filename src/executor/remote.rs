//! Remote task execution via P2P network.

use std::sync::Arc;
use std::time::{Duration, Instant};

use libp2p::PeerId;
use tokio::sync::{mpsc, RwLock};
use tokio::time::timeout;

use crate::job::{
    network::{self, JobMessage, JobRequestMessage, JobBidMessage, BidAcceptedMessage, JobResultMessage},
    Job, JobBid, JobId, JobManager, JobRequest, JobResult, ResourceType,
};
use crate::p2p::{Network, NetworkEvent};
use crate::wallet::to_micro;

use super::router::PeerFilter;
use super::task::{ExecutionTask, TaskData, TaskMetrics, TaskResult, ExecutionLocation, TaskId};
use super::ExecutorError;

/// Handles remote task execution via the P2P network.
pub struct RemoteExecutor {
    job_manager: Arc<RwLock<JobManager>>,
    network: Arc<RwLock<Network>>,
    local_peer_id: PeerId,
    config: RemoteExecutorConfig,
}

impl RemoteExecutor {
    /// Create a new remote executor.
    pub fn new(
        job_manager: Arc<RwLock<JobManager>>,
        network: Arc<RwLock<Network>>,
        local_peer_id: PeerId,
        config: RemoteExecutorConfig,
    ) -> Self {
        Self {
            job_manager,
            network,
            local_peer_id,
            config,
        }
    }

    /// Execute a task on a remote peer.
    pub async fn execute(
        &self,
        task_id: TaskId,
        task: ExecutionTask,
        peer_filter: PeerFilter,
    ) -> Result<TaskResult, ExecutorError> {
        let start = Instant::now();

        // Convert task to job request
        let request = self.task_to_request(&task)?;
        let job_id = request.id.clone();

        // Broadcast job request
        self.broadcast_request(&request).await?;

        // Wait for bids
        let bids = self.collect_bids(&job_id, &peer_filter).await?;

        if bids.is_empty() {
            return Err(ExecutorError::InsufficientResources(
                "No peers available to handle task".to_string(),
            ));
        }

        // Select best bid
        let best_bid = self.select_best_bid(&bids);

        // Accept bid and wait for result
        let result = self.execute_with_bid(&job_id, &best_bid).await?;

        let total_time_ms = start.elapsed().as_millis() as u64;

        Ok(TaskResult {
            task_id,
            location: ExecutionLocation::Remote {
                peer_id: best_bid.bidder_id.clone(),
                job_id,
            },
            data: self.result_to_task_data(&task, &result),
            metrics: TaskMetrics {
                ttfb_ms: result.metrics.ttfb_ms,
                total_time_ms,
                queue_time_ms: 0,
            },
            cost: Some(best_bid.price),
        })
    }

    /// Convert a task to a job request.
    fn task_to_request(&self, task: &ExecutionTask) -> Result<JobRequest, ExecutorError> {
        let (resource_type, max_budget, timeout_secs) = match task {
            ExecutionTask::Inference(t) => (
                ResourceType::Inference {
                    model: t.model.clone(),
                    tokens: t.max_tokens,
                },
                to_micro(10.0), // Default budget
                300,
            ),
            ExecutionTask::WebFetch(t) => (
                ResourceType::WebFetch { url_count: 1 },
                to_micro(1.0),
                t.timeout_secs as u64,
            ),
            ExecutionTask::WebSearch(_t) => (
                ResourceType::WebFetch { url_count: 10 }, // Search returns multiple URLs
                to_micro(2.0),
                60,
            ),
            ExecutionTask::WasmExecution(t) => (
                ResourceType::WasmTool {
                    tool_name: t.tool_hash.clone(),
                    invocations: 1,
                },
                to_micro(5.0),
                t.timeout_secs as u64,
            ),
        };

        let mut request = JobRequest::new(resource_type, max_budget, timeout_secs);

        // Add task payload
        let payload = rmp_serde::to_vec(&task)
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;
        request = request.with_payload(payload);

        Ok(request)
    }

    /// Broadcast a job request to the network.
    async fn broadcast_request(&self, request: &JobRequest) -> Result<(), ExecutorError> {
        let message = JobMessage::Request(JobRequestMessage::new(
            request.clone(),
            &self.local_peer_id,
        ));

        let data = network::serialize_message(&message)
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        // Store locally
        self.job_manager
            .write()
            .await
            .create_request(request.clone())
            .await
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        // Broadcast to network
        self.network
            .write()
            .await
            .publish(network::topics::JOB_REQUESTS, data)
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        tracing::info!(job_id = %request.id, "Broadcast job request to network");

        Ok(())
    }

    /// Collect bids for a job request.
    async fn collect_bids(
        &self,
        job_id: &JobId,
        _peer_filter: &PeerFilter,
    ) -> Result<Vec<JobBid>, ExecutorError> {
        // Wait for bids to arrive
        let bid_deadline = Duration::from_secs(self.config.bid_collection_timeout_secs);

        tokio::time::sleep(bid_deadline).await;

        // Get collected bids
        let bids = self.job_manager.read().await.get_bids(job_id).await;

        tracing::info!(
            job_id = %job_id,
            bid_count = bids.len(),
            "Collected bids for job"
        );

        Ok(bids)
    }

    /// Select the best bid based on price, latency, and reputation.
    fn select_best_bid(&self, bids: &[JobBid]) -> JobBid {
        // Use the scoring mechanism from JobBid
        bids.iter()
            .max_by(|a, b| {
                let score_a = a.score(Some(100)); // 100ms target latency
                let score_b = b.score(Some(100));
                score_a.partial_cmp(&score_b).unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
            .expect("bids should not be empty")
    }

    /// Execute job with the accepted bid.
    async fn execute_with_bid(
        &self,
        job_id: &JobId,
        bid: &JobBid,
    ) -> Result<JobResult, ExecutorError> {
        // Accept the bid
        let job = self
            .job_manager
            .write()
            .await
            .accept_bid(job_id, &bid.id)
            .await
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        // Notify the winner
        let accept_msg = JobMessage::BidAccepted(BidAcceptedMessage {
            job_id: job_id.clone(),
            bid_id: bid.id.0.clone(),
            winner_peer_id: bid.bidder_id.clone(),
            escrow_id: job.escrow_id.0.clone(),
            signature: vec![],
        });

        let data = network::serialize_message(&accept_msg)
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        self.network
            .write()
            .await
            .publish(network::topics::JOB_STATUS, data)
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        // Wait for result
        let result_timeout = Duration::from_secs(self.config.result_timeout_secs);

        let result = timeout(result_timeout, self.wait_for_result(job_id))
            .await
            .map_err(|_| ExecutorError::Timeout)?
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        // Settle the job
        self.job_manager
            .write()
            .await
            .settle_job(job_id, true)
            .await
            .map_err(|e| ExecutorError::InferenceError(e.to_string()))?;

        Ok(result)
    }

    /// Wait for a job result to arrive.
    async fn wait_for_result(&self, job_id: &JobId) -> Result<JobResult, String> {
        // Poll for result
        loop {
            let job = self.job_manager.read().await.get_job(job_id).await;

            if let Some(job) = job {
                if let Some(result) = job.result {
                    return Ok(result);
                }
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Convert a job result to task data.
    fn result_to_task_data(&self, task: &ExecutionTask, result: &JobResult) -> TaskData {
        match task {
            ExecutionTask::Inference(_) => {
                // Parse inference result from job result data
                TaskData::Inference(super::task::InferenceResult {
                    text: String::from_utf8_lossy(&result.data).to_string(),
                    tokens_generated: result.actual_usage.tokens.unwrap_or(0),
                    tokens_per_second: result.metrics.tokens_per_sec.unwrap_or(0.0),
                    finish_reason: super::task::FinishReason::Stop,
                })
            }
            ExecutionTask::WebFetch(_) => {
                TaskData::WebFetch(super::task::WebFetchResult {
                    status: 200,
                    headers: vec![],
                    body: result.data.clone(),
                })
            }
            ExecutionTask::WebSearch(_) => {
                TaskData::WebSearch(super::task::WebSearchResult {
                    results: vec![],
                })
            }
            ExecutionTask::WasmExecution(_) => {
                TaskData::Wasm(super::task::WasmResult {
                    value: serde_json::from_slice(&result.data).unwrap_or_default(),
                    fuel_consumed: 0,
                })
            }
        }
    }
}

/// Configuration for remote execution.
#[derive(Debug, Clone)]
pub struct RemoteExecutorConfig {
    /// Timeout for collecting bids in seconds
    pub bid_collection_timeout_secs: u64,
    /// Timeout for waiting for result in seconds
    pub result_timeout_secs: u64,
    /// Maximum price multiplier above estimate
    pub max_price_multiplier: f64,
}

impl Default for RemoteExecutorConfig {
    fn default() -> Self {
        Self {
            bid_collection_timeout_secs: 10,
            result_timeout_secs: 300,
            max_price_multiplier: 2.0,
        }
    }
}

/// Provider-side handler for executing jobs from the network.
pub struct JobProvider {
    job_manager: Arc<RwLock<JobManager>>,
    network: Arc<RwLock<Network>>,
    local_peer_id: PeerId,
    /// Pending requests we've bid on (stored for execution when accepted)
    pending_requests: RwLock<std::collections::HashMap<JobId, JobRequest>>,
}

impl JobProvider {
    pub fn new(
        job_manager: Arc<RwLock<JobManager>>,
        network: Arc<RwLock<Network>>,
        local_peer_id: PeerId,
    ) -> Self {
        Self {
            job_manager,
            network,
            local_peer_id,
            pending_requests: RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Handle an incoming job request.
    pub async fn handle_request(&self, msg: JobRequestMessage) -> Result<(), String> {
        // Don't bid on our own requests
        if msg.requester_peer_id == self.local_peer_id.to_string() {
            return Ok(());
        }

        // Evaluate if we want to bid
        let bid = self
            .job_manager
            .write()
            .await
            .evaluate_request(&msg.request)
            .await;

        if let Some(mut bid) = bid {
            // Update with actual peer ID
            bid.bidder_id = self.local_peer_id.to_string();

            // Store the request so we can execute it later if our bid is accepted
            self.pending_requests.write().await.insert(msg.request.id.clone(), msg.request.clone());

            // Send bid
            let bid_msg = JobMessage::Bid(JobBidMessage::new(bid.clone(), &self.local_peer_id));
            let data = network::serialize_message(&bid_msg)
                .map_err(|e| e.to_string())?;

            self.network
                .write()
                .await
                .publish(network::topics::JOB_BIDS, data)
                .map_err(|e| e.to_string())?;

            tracing::info!(
                job_id = %msg.request.id,
                price = bid.price,
                "Submitted bid for job"
            );
        }

        Ok(())
    }

    /// Handle notification that our bid was accepted.
    pub async fn handle_bid_accepted(&self, msg: BidAcceptedMessage) -> Result<(), String> {
        if msg.winner_peer_id != self.local_peer_id.to_string() {
            return Ok(()); // Not for us
        }

        tracing::info!(job_id = %msg.job_id, "Our bid was accepted, preparing to execute job");

        // Get the request we bid on
        let request = self.pending_requests.write().await.remove(&msg.job_id);

        if let Some(request) = request {
            tracing::info!(
                job_id = %msg.job_id,
                resource_type = %request.resource_type,
                "Found request, will execute when provider calls execute_accepted_job"
            );
            // Note: Actual execution happens in serve.rs via execute_job_locally
            // because we need access to the TaskExecutor which isn't available here
        } else {
            tracing::warn!(job_id = %msg.job_id, "Request not found in pending requests");
        }

        Ok(())
    }

    /// Get a pending request by job ID (for execution).
    pub async fn get_pending_request(&self, job_id: &JobId) -> Option<JobRequest> {
        self.pending_requests.read().await.get(job_id).cloned()
    }

    /// Remove a pending request after execution.
    pub async fn remove_pending_request(&self, job_id: &JobId) -> Option<JobRequest> {
        self.pending_requests.write().await.remove(job_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_executor_config() {
        let config = RemoteExecutorConfig::default();
        assert_eq!(config.bid_collection_timeout_secs, 10);
    }
}
