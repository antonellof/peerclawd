//! Job execution and settlement.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use super::{JobBid, JobId, JobRequest};
use crate::wallet::EscrowId;

/// Status of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job created, bid accepted, awaiting execution
    Pending,
    /// Job is being executed
    InProgress,
    /// Result submitted, awaiting verification
    PendingVerification,
    /// Job completed successfully
    Completed,
    /// Job failed
    Failed,
    /// Job timed out
    TimedOut,
    /// Job cancelled
    Cancelled,
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobStatus::Pending => write!(f, "Pending"),
            JobStatus::InProgress => write!(f, "In Progress"),
            JobStatus::PendingVerification => write!(f, "Pending Verification"),
            JobStatus::Completed => write!(f, "Completed"),
            JobStatus::Failed => write!(f, "Failed"),
            JobStatus::TimedOut => write!(f, "Timed Out"),
            JobStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// Result of a completed job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    /// Result data (interpretation depends on job type)
    pub data: Vec<u8>,
    /// Hash of the result for verification
    pub hash: String,
    /// Actual resource usage
    pub actual_usage: ActualUsage,
    /// Execution metrics
    pub metrics: ExecutionMetrics,
}

impl JobResult {
    /// Create a new job result.
    pub fn new(data: Vec<u8>) -> Self {
        let hash = blake3::hash(&data).to_hex().to_string();
        Self {
            data,
            hash,
            actual_usage: ActualUsage::default(),
            metrics: ExecutionMetrics::default(),
        }
    }

    /// Set actual usage.
    pub fn with_usage(mut self, usage: ActualUsage) -> Self {
        self.actual_usage = usage;
        self
    }

    /// Set execution metrics.
    pub fn with_metrics(mut self, metrics: ExecutionMetrics) -> Self {
        self.metrics = metrics;
        self
    }
}

/// Actual resource usage during job execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActualUsage {
    /// Actual tokens processed (for inference/embedding)
    pub tokens: Option<u32>,
    /// Actual compute time in milliseconds
    pub compute_time_ms: Option<u64>,
    /// Actual bytes processed
    pub bytes: Option<u64>,
}

/// Execution performance metrics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionMetrics {
    /// Time to first byte in milliseconds
    pub ttfb_ms: u64,
    /// Total execution time in milliseconds
    pub total_time_ms: u64,
    /// Tokens per second (for inference)
    pub tokens_per_sec: Option<f64>,
}

/// An active job being executed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique identifier (same as request ID)
    pub id: JobId,
    /// Original request
    pub request: JobRequest,
    /// Winning bid
    pub bid: JobBid,
    /// Escrow holding payment
    pub escrow_id: EscrowId,
    /// Current status
    pub status: JobStatus,
    /// When the job was created (bid accepted)
    pub created_at: DateTime<Utc>,
    /// When execution started
    pub started_at: Option<DateTime<Utc>>,
    /// When execution completed
    pub completed_at: Option<DateTime<Utc>>,
    /// Job result (if completed)
    pub result: Option<JobResult>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Number of verification attempts
    pub verification_attempts: u32,
}

impl Job {
    /// Create a new job from accepted bid.
    pub fn new(request: JobRequest, bid: JobBid, escrow_id: EscrowId) -> Self {
        Self {
            id: request.id.clone(),
            request,
            bid,
            escrow_id,
            status: JobStatus::Pending,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            verification_attempts: 0,
        }
    }

    /// Check if the job has timed out.
    pub fn is_timed_out(&self) -> bool {
        let deadline = self.created_at + chrono::Duration::seconds(self.request.timeout_secs as i64);
        Utc::now() > deadline
    }

    /// Get remaining time until timeout.
    pub fn time_remaining(&self) -> chrono::Duration {
        let deadline = self.created_at + chrono::Duration::seconds(self.request.timeout_secs as i64);
        deadline - Utc::now()
    }

    /// Get execution duration (if started).
    pub fn execution_duration(&self) -> Option<chrono::Duration> {
        self.started_at.map(|start| {
            self.completed_at.unwrap_or_else(Utc::now) - start
        })
    }

    /// Check if job is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::TimedOut | JobStatus::Cancelled
        )
    }

    /// Get price in PCLAW.
    pub fn price_pclaw(&self) -> f64 {
        crate::wallet::from_micro(self.bid.price)
    }
}

impl fmt::Display for Job {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Job[{}]: {} → {} ({:.6} PCLAW) [{}]",
            self.id,
            self.request.resource_type,
            self.bid.bidder_id,
            self.price_pclaw(),
            self.status
        )
    }
}

/// Verification result for a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Whether the result passed verification
    pub passed: bool,
    /// Confidence score (0.0 - 1.0)
    pub confidence: f64,
    /// Verification method used
    pub method: VerificationMethod,
    /// Optional details
    pub details: Option<String>,
}

/// Method used for verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationMethod {
    /// No verification (trusted peer)
    None,
    /// Hash comparison
    Hash,
    /// Redundant execution with comparison
    Redundant,
    /// Spot-check sampling
    Sampling,
    /// Manual review
    Manual,
}

impl VerificationResult {
    /// Create a passing result.
    pub fn pass(method: VerificationMethod) -> Self {
        Self {
            passed: true,
            confidence: 1.0,
            method,
            details: None,
        }
    }

    /// Create a failing result.
    pub fn fail(method: VerificationMethod, details: String) -> Self {
        Self {
            passed: false,
            confidence: 1.0,
            method,
            details: Some(details),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobBid, JobRequest, ResourceType};
    use crate::wallet::to_micro;

    fn create_test_job() -> Job {
        let request = JobRequest::new(
            ResourceType::Inference {
                model: "llama-3.2-8b".into(),
                tokens: 1000,
            },
            to_micro(10.0),
            300,
        );

        let bid = JobBid::new(
            request.id.clone(),
            "peer_123".to_string(),
            to_micro(5.0),
            100,
            60,
        );

        Job::new(request, bid, EscrowId::new())
    }

    #[test]
    fn test_job_creation() {
        let job = create_test_job();

        assert_eq!(job.status, JobStatus::Pending);
        assert!(!job.is_terminal());
        assert!(!job.is_timed_out());
    }

    #[test]
    fn test_job_timeout() {
        let mut job = create_test_job();
        job.request.timeout_secs = 0;
        job.created_at = Utc::now() - chrono::Duration::seconds(10);

        assert!(job.is_timed_out());
    }

    #[test]
    fn test_job_result() {
        let data = b"test result data".to_vec();
        let result = JobResult::new(data.clone());

        assert_eq!(result.data, data);
        assert!(!result.hash.is_empty());
    }

    #[test]
    fn test_verification_result() {
        let pass = VerificationResult::pass(VerificationMethod::Hash);
        assert!(pass.passed);

        let fail = VerificationResult::fail(
            VerificationMethod::Redundant,
            "Results mismatch".into(),
        );
        assert!(!fail.passed);
    }
}
