//! Job system for resource marketplace.
//!
//! Implements the request → bid → accept → execute → settle flow
//! for trading compute resources on the network.

mod pricing;
mod request;
pub mod bid;
pub mod execution;
pub mod network;

pub use pricing::{ResourceType, ResourcePricing, PricingStrategy};
pub use request::{JobRequest, JobRequirements, JobId};
pub use bid::{JobBid, BidId, BidStatus, select_best_bid};
pub use execution::{Job, JobStatus, JobResult, ActualUsage, ExecutionMetrics};
pub use network::{JobMessage, topics as job_topics};

use crate::wallet::{Wallet, WalletError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

#[derive(Error, Debug)]
pub enum JobError {
    #[error("Job not found: {0}")]
    NotFound(JobId),

    #[error("Bid not found: {0}")]
    BidNotFound(BidId),

    #[error("Invalid state transition: {0}")]
    InvalidState(String),

    #[error("Insufficient budget: need {required}, have {available}")]
    InsufficientBudget { required: u64, available: u64 },

    #[error("Bid expired")]
    BidExpired,

    #[error("Job timeout")]
    Timeout,

    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    #[error("Wallet error: {0}")]
    Wallet(#[from] WalletError),

    #[error("Serialization error: {0}")]
    Serialization(String),
}

/// The job manager handles the lifecycle of jobs.
pub struct JobManager {
    /// Local peer ID for identifying this node in bids
    local_peer_id: String,
    /// Active job requests awaiting bids
    requests: RwLock<HashMap<JobId, JobRequest>>,
    /// Bids received for requests
    bids: RwLock<HashMap<JobId, Vec<JobBid>>>,
    /// Active jobs being executed
    active_jobs: RwLock<HashMap<JobId, Job>>,
    /// Completed jobs (recent history)
    completed_jobs: RwLock<Vec<Job>>,
    /// Local peer's pricing strategy
    pricing: RwLock<PricingStrategy>,
    /// Reference to wallet for escrow
    wallet: Arc<Wallet>,
}

impl JobManager {
    /// Create a new job manager.
    pub fn new(wallet: Arc<Wallet>, local_peer_id: String) -> Self {
        Self {
            local_peer_id,
            requests: RwLock::new(HashMap::new()),
            bids: RwLock::new(HashMap::new()),
            active_jobs: RwLock::new(HashMap::new()),
            completed_jobs: RwLock::new(Vec::new()),
            pricing: RwLock::new(PricingStrategy::default()),
            wallet,
        }
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }

    /// Set the local pricing strategy.
    pub async fn set_pricing(&self, pricing: PricingStrategy) {
        *self.pricing.write().await = pricing;
    }

    /// Get the current pricing strategy.
    pub async fn get_pricing(&self) -> PricingStrategy {
        self.pricing.read().await.clone()
    }

    // =========================================================================
    // Requester Side (Agent wanting resources)
    // =========================================================================

    /// Create a new job request and broadcast to the network.
    pub async fn create_request(&self, request: JobRequest) -> Result<JobId, JobError> {
        let job_id = request.id.clone();

        // Store locally
        self.requests.write().await.insert(job_id.clone(), request);
        self.bids.write().await.insert(job_id.clone(), Vec::new());

        // TODO: Broadcast to network via GossipSub

        Ok(job_id)
    }

    /// Receive a bid for one of our requests.
    pub async fn receive_bid(&self, bid: JobBid) -> Result<(), JobError> {
        let mut bids = self.bids.write().await;

        if let Some(job_bids) = bids.get_mut(&bid.job_id) {
            job_bids.push(bid);
            Ok(())
        } else {
            Err(JobError::NotFound(bid.job_id))
        }
    }

    /// Get all bids for a request.
    pub async fn get_bids(&self, job_id: &JobId) -> Vec<JobBid> {
        self.bids.read().await
            .get(job_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Accept a bid and create escrow for the job.
    pub async fn accept_bid(&self, job_id: &JobId, bid_id: &BidId) -> Result<Job, JobError> {
        // Get the request and bid
        let request = self.requests.read().await
            .get(job_id)
            .cloned()
            .ok_or_else(|| JobError::NotFound(job_id.clone()))?;

        let bid = {
            let bids = self.bids.read().await;
            bids.get(job_id)
                .and_then(|b| b.iter().find(|b| &b.id == bid_id).cloned())
                .ok_or_else(|| JobError::BidNotFound(bid_id.clone()))?
        };

        // Check bid hasn't expired
        if bid.is_expired() {
            return Err(JobError::BidExpired);
        }

        // Create escrow for the bid amount
        let escrow = self.wallet.create_escrow(
            bid.price,
            bid.bidder_id.clone(),
            job_id.0.clone(),
            request.timeout_secs,
        ).await?;

        // Create the active job
        let job = Job::new(request, bid, escrow.id);

        // Move from requests to active
        self.requests.write().await.remove(job_id);
        self.bids.write().await.remove(job_id);
        self.active_jobs.write().await.insert(job_id.clone(), job.clone());

        // TODO: Notify the winning bidder via P2P

        Ok(job)
    }

    // =========================================================================
    // Provider Side (Peer offering resources)
    // =========================================================================

    /// Evaluate a job request and decide whether to bid.
    pub async fn evaluate_request(&self, request: &JobRequest) -> Option<JobBid> {
        let pricing = self.pricing.read().await;

        // Check if we can fulfill the requirements
        // TODO: Check actual local resources

        // Calculate our price
        let price = pricing.calculate_price(&request.resource_type, request.units);

        // Check if our price is within the requester's budget
        if price > request.max_budget {
            return None;
        }

        // Create a bid
        Some(JobBid::new(
            request.id.clone(),
            self.local_peer_id.clone(),
            price,
            pricing.estimated_latency_ms,
            60, // Bid valid for 60 seconds
        ))
    }

    /// Submit a bid for a job request.
    pub async fn submit_bid(&self, request: &JobRequest) -> Result<Option<JobBid>, JobError> {
        if let Some(bid) = self.evaluate_request(request).await {
            // TODO: Send bid to requester via P2P
            Ok(Some(bid))
        } else {
            Ok(None)
        }
    }

    /// Handle notification that our bid was accepted.
    pub async fn bid_accepted(&self, job: Job) -> Result<(), JobError> {
        self.active_jobs.write().await.insert(job.id.clone(), job);
        Ok(())
    }

    // =========================================================================
    // Execution (Both sides)
    // =========================================================================

    /// Get an active job by ID.
    pub async fn get_job(&self, job_id: &JobId) -> Option<Job> {
        self.active_jobs.read().await.get(job_id).cloned()
    }

    /// Mark a job as in progress.
    pub async fn start_job(&self, job_id: &JobId) -> Result<(), JobError> {
        let mut jobs = self.active_jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.status = JobStatus::InProgress;
            job.started_at = Some(Utc::now());
            Ok(())
        } else {
            Err(JobError::NotFound(job_id.clone()))
        }
    }

    /// Submit a job result.
    pub async fn submit_result(&self, job_id: &JobId, result: JobResult) -> Result<(), JobError> {
        let mut jobs = self.active_jobs.write().await;
        if let Some(job) = jobs.get_mut(job_id) {
            job.result = Some(result);
            job.status = JobStatus::PendingVerification;
            Ok(())
        } else {
            Err(JobError::NotFound(job_id.clone()))
        }
    }

    /// Verify and settle a completed job.
    pub async fn settle_job(&self, job_id: &JobId, success: bool) -> Result<(), JobError> {
        let job = {
            let mut jobs = self.active_jobs.write().await;
            jobs.remove(job_id)
                .ok_or_else(|| JobError::NotFound(job_id.clone()))?
        };

        if success {
            // Release escrow to worker
            self.wallet.release_escrow(&job.escrow_id).await?;
        } else {
            // Refund escrow to requester
            self.wallet.refund_escrow(&job.escrow_id).await?;
        }

        // Move to completed
        let mut completed_job = job;
        completed_job.status = if success {
            JobStatus::Completed
        } else {
            JobStatus::Failed
        };
        completed_job.completed_at = Some(Utc::now());

        let mut completed = self.completed_jobs.write().await;
        completed.push(completed_job);

        // Trim old completed jobs (keep last 100)
        if completed.len() > 100 {
            let len = completed.len();
            completed.drain(0..len - 100);
        }

        Ok(())
    }

    /// Get active jobs.
    pub async fn active_jobs(&self) -> Vec<Job> {
        self.active_jobs.read().await.values().cloned().collect()
    }

    /// Get recent completed jobs.
    pub async fn completed_jobs(&self, limit: usize) -> Vec<Job> {
        let jobs = self.completed_jobs.read().await;
        let len = jobs.len();
        let start = len.saturating_sub(limit);
        jobs[start..].to_vec()
    }

    /// Get pending job requests (awaiting bids).
    pub async fn pending_requests(&self) -> Vec<JobRequest> {
        self.requests.read().await.values().cloned().collect()
    }

    /// Get bids count for a pending request.
    pub async fn bids_count(&self, job_id: &JobId) -> usize {
        self.bids.read().await
            .get(job_id)
            .map(|b| b.len())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::wallet::WalletConfig;
    use tempfile::tempdir;

    async fn setup_job_manager() -> (JobManager, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.redb")).unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let peer_id = identity.peer_id().to_string();
        let wallet = Arc::new(
            Wallet::new(identity, WalletConfig::default(), db).unwrap()
        );

        // Credit some tokens for testing
        wallet.credit(crate::wallet::to_micro(1000.0), "test").await.unwrap();

        (JobManager::new(wallet, peer_id), dir)
    }

    #[tokio::test]
    async fn test_create_request() {
        let (manager, _dir) = setup_job_manager().await;

        let request = JobRequest::new(
            ResourceType::Inference { model: "llama-3.2-8b".into(), tokens: 1000 },
            crate::wallet::to_micro(10.0),
            300,
        );

        let job_id = manager.create_request(request).await.unwrap();
        assert!(!job_id.0.is_empty());
    }

    #[tokio::test]
    async fn test_bid_flow() {
        let (manager, _dir) = setup_job_manager().await;

        // Create request
        let request = JobRequest::new(
            ResourceType::Inference { model: "llama-3.2-8b".into(), tokens: 1000 },
            crate::wallet::to_micro(10.0),
            300,
        );
        let job_id = manager.create_request(request).await.unwrap();

        // Receive a bid
        let bid = JobBid::new(
            job_id.clone(),
            "peer_123".to_string(),
            crate::wallet::to_micro(5.0),
            100,
            60,
        );
        manager.receive_bid(bid.clone()).await.unwrap();

        // Check bids
        let bids = manager.get_bids(&job_id).await;
        assert_eq!(bids.len(), 1);

        // Accept bid
        let job = manager.accept_bid(&job_id, &bid.id).await.unwrap();
        assert_eq!(job.status, JobStatus::Pending);
    }
}
