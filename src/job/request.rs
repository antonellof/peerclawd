//! Job request types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use super::ResourceType;

/// Unique identifier for a job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(pub String);

impl JobId {
    /// Generate a new random job ID.
    pub fn new() -> Self {
        Self(format!("job_{}", Uuid::new_v4().to_string().replace("-", "")))
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for JobId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Requirements for job execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequirements {
    /// Maximum acceptable latency in milliseconds
    pub max_latency_ms: Option<u32>,
    /// Minimum provider reputation score (0.0 - 1.0)
    pub min_reputation: Option<f64>,
    /// Required capabilities
    pub capabilities: Vec<String>,
    /// Preferred peer IDs (optional)
    pub preferred_peers: Vec<String>,
    /// Excluded peer IDs
    pub excluded_peers: Vec<String>,
}

impl Default for JobRequirements {
    fn default() -> Self {
        Self {
            max_latency_ms: None,
            min_reputation: Some(0.3), // Exclude untrusted peers by default
            capabilities: Vec::new(),
            preferred_peers: Vec::new(),
            excluded_peers: Vec::new(),
        }
    }
}

/// A request for resources from an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRequest {
    /// Unique identifier
    pub id: JobId,
    /// Type of resource requested
    pub resource_type: ResourceType,
    /// Number of units (interpretation depends on resource type)
    pub units: u32,
    /// Maximum budget in μPCLAW
    pub max_budget: u64,
    /// Job requirements
    pub requirements: JobRequirements,
    /// Timeout for job completion in seconds
    pub timeout_secs: u64,
    /// When the request was created
    pub created_at: DateTime<Utc>,
    /// When the request expires (no more bids accepted)
    pub expires_at: DateTime<Utc>,
    /// Requester's peer ID
    pub requester_id: String,
    /// Optional payload data (e.g., prompt for inference)
    pub payload: Option<Vec<u8>>,
}

impl JobRequest {
    /// Create a new job request.
    pub fn new(resource_type: ResourceType, max_budget: u64, timeout_secs: u64) -> Self {
        let now = Utc::now();
        let units = Self::extract_units(&resource_type);

        Self {
            id: JobId::new(),
            resource_type,
            units,
            max_budget,
            requirements: JobRequirements::default(),
            timeout_secs,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(60), // 60 second bid window
            requester_id: String::new(), // Set by caller
            payload: None,
        }
    }

    /// Create a request with custom requirements.
    pub fn with_requirements(mut self, requirements: JobRequirements) -> Self {
        self.requirements = requirements;
        self
    }

    /// Set the requester ID.
    pub fn with_requester(mut self, requester_id: String) -> Self {
        self.requester_id = requester_id;
        self
    }

    /// Set the payload.
    pub fn with_payload(mut self, payload: Vec<u8>) -> Self {
        self.payload = Some(payload);
        self
    }

    /// Set bid expiration time.
    pub fn with_bid_window(mut self, seconds: u64) -> Self {
        self.expires_at = self.created_at + chrono::Duration::seconds(seconds as i64);
        self
    }

    /// Check if the request has expired (no more bids accepted).
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Extract units from resource type for pricing.
    fn extract_units(resource: &ResourceType) -> u32 {
        match resource {
            ResourceType::Inference { tokens, .. } => *tokens,
            ResourceType::Embedding { tokens, .. } => *tokens,
            ResourceType::ImageGeneration { count, .. } => *count,
            ResourceType::Cpu { cores, duration_secs } => (*cores as u64 * duration_secs) as u32,
            ResourceType::Gpu { duration_secs, .. } => *duration_secs as u32,
            ResourceType::Storage { bytes, .. } => (*bytes / 1024) as u32, // KB
            ResourceType::WebFetch { url_count } => *url_count,
            ResourceType::VectorSearch { query_count } => *query_count,
            ResourceType::WasmTool { invocations, .. } => *invocations,
        }
    }

    /// Get budget in PCLAW.
    pub fn budget_pclaw(&self) -> f64 {
        crate::wallet::from_micro(self.max_budget)
    }
}

impl fmt::Display for JobRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "JobRequest[{}]: {} (budget: {:.6} PCLAW, timeout: {}s)",
            self.id,
            self.resource_type,
            self.budget_pclaw(),
            self.timeout_secs
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::to_micro;

    #[test]
    fn test_job_request_creation() {
        let request = JobRequest::new(
            ResourceType::Inference {
                model: "llama-3.2-8b".into(),
                tokens: 1000,
            },
            to_micro(10.0),
            300,
        );

        assert!(!request.id.0.is_empty());
        assert_eq!(request.units, 1000);
        assert_eq!(request.timeout_secs, 300);
    }

    #[test]
    fn test_job_request_expiry() {
        let mut request = JobRequest::new(
            ResourceType::WebFetch { url_count: 5 },
            to_micro(1.0),
            60,
        );

        // Should not be expired initially
        assert!(!request.is_expired());

        // Set to past
        request.expires_at = Utc::now() - chrono::Duration::seconds(10);
        assert!(request.is_expired());
    }

    #[test]
    fn test_job_id_uniqueness() {
        let id1 = JobId::new();
        let id2 = JobId::new();
        assert_ne!(id1, id2);
    }
}
