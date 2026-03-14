//! Job bid types.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use super::JobId;

/// Unique identifier for a bid.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BidId(pub String);

impl BidId {
    /// Generate a new random bid ID.
    pub fn new() -> Self {
        Self(format!("bid_{}", Uuid::new_v4().to_string().replace("-", "")))
    }
}

impl Default for BidId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for BidId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for BidId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Status of a bid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BidStatus {
    /// Bid is active and awaiting decision
    Pending,
    /// Bid was accepted
    Accepted,
    /// Bid was rejected (another bid chosen)
    Rejected,
    /// Bid expired before decision
    Expired,
    /// Bid was withdrawn by bidder
    Withdrawn,
}

impl fmt::Display for BidStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BidStatus::Pending => write!(f, "Pending"),
            BidStatus::Accepted => write!(f, "Accepted"),
            BidStatus::Rejected => write!(f, "Rejected"),
            BidStatus::Expired => write!(f, "Expired"),
            BidStatus::Withdrawn => write!(f, "Withdrawn"),
        }
    }
}

/// A bid from a peer to fulfill a job request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobBid {
    /// Unique identifier
    pub id: BidId,
    /// Job being bid on
    pub job_id: JobId,
    /// Peer making the bid
    pub bidder_id: String,
    /// Offered price in μPCLAW
    pub price: u64,
    /// Estimated completion time in milliseconds
    pub estimated_latency_ms: u32,
    /// When the bid was submitted
    pub created_at: DateTime<Utc>,
    /// When the bid expires
    pub expires_at: DateTime<Utc>,
    /// Current status
    pub status: BidStatus,
    /// Bidder's reputation score at time of bid
    pub reputation: f64,
    /// Optional message from bidder
    pub message: Option<String>,
}

impl JobBid {
    /// Create a new bid.
    pub fn new(
        job_id: JobId,
        bidder_id: String,
        price: u64,
        estimated_latency_ms: u32,
        valid_secs: u64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: BidId::new(),
            job_id,
            bidder_id,
            price,
            estimated_latency_ms,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(valid_secs as i64),
            status: BidStatus::Pending,
            reputation: 0.5, // Default, should be set by caller
            message: None,
        }
    }

    /// Set the reputation score.
    pub fn with_reputation(mut self, reputation: f64) -> Self {
        self.reputation = reputation;
        self
    }

    /// Set an optional message.
    pub fn with_message(mut self, message: String) -> Self {
        self.message = Some(message);
        self
    }

    /// Check if the bid has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if the bid is still valid (pending and not expired).
    pub fn is_valid(&self) -> bool {
        self.status == BidStatus::Pending && !self.is_expired()
    }

    /// Get price in PCLAW.
    pub fn price_pclaw(&self) -> f64 {
        crate::wallet::from_micro(self.price)
    }

    /// Calculate a score for bid comparison.
    /// Higher score = better bid.
    pub fn score(&self, max_latency_ms: Option<u32>) -> f64 {
        // Components:
        // - Price (lower is better)
        // - Latency (lower is better, if within SLA)
        // - Reputation (higher is better)

        let mut score = 0.0;

        // Reputation weight: 40%
        score += self.reputation * 40.0;

        // Latency weight: 30% (if within SLA)
        if let Some(max_lat) = max_latency_ms {
            if self.estimated_latency_ms <= max_lat {
                // Score based on how much faster than SLA
                let lat_score = 1.0 - (self.estimated_latency_ms as f64 / max_lat as f64);
                score += lat_score * 30.0;
            }
            // If over SLA, no latency points
        } else {
            // No SLA specified, just use inverse latency
            score += (1000.0 / self.estimated_latency_ms as f64).min(30.0);
        }

        // Price weight: 30% (relative, caller should normalize)
        // For now, we'll use inverse price (lower price = higher score)
        // This will be normalized when comparing bids
        score += 30.0 / (1.0 + self.price as f64 / 1_000_000.0);

        score
    }
}

impl PartialEq for JobBid {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for JobBid {}

impl fmt::Display for JobBid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Bid[{}]: {:.6} PCLAW, {}ms latency, rep={:.2} ({})",
            self.id,
            self.price_pclaw(),
            self.estimated_latency_ms,
            self.reputation,
            self.status
        )
    }
}

/// Select the best bid from a list.
pub fn select_best_bid(bids: &[JobBid], max_latency_ms: Option<u32>) -> Option<&JobBid> {
    bids.iter()
        .filter(|b| b.is_valid())
        .filter(|b| {
            // Filter by latency SLA if specified
            max_latency_ms.map_or(true, |max| b.estimated_latency_ms <= max)
        })
        .max_by(|a, b| {
            a.score(max_latency_ms)
                .partial_cmp(&b.score(max_latency_ms))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::to_micro;

    #[test]
    fn test_bid_creation() {
        let bid = JobBid::new(
            JobId::new(),
            "peer_123".to_string(),
            to_micro(5.0),
            100,
            60,
        );

        assert!(bid.is_valid());
        assert_eq!(bid.status, BidStatus::Pending);
    }

    #[test]
    fn test_bid_expiry() {
        let mut bid = JobBid::new(
            JobId::new(),
            "peer_123".to_string(),
            to_micro(5.0),
            100,
            60,
        );

        assert!(!bid.is_expired());

        bid.expires_at = Utc::now() - chrono::Duration::seconds(10);
        assert!(bid.is_expired());
        assert!(!bid.is_valid());
    }

    #[test]
    fn test_bid_selection() {
        let job_id = JobId::new();

        let bid1 = JobBid::new(job_id.clone(), "peer_1".into(), to_micro(10.0), 200, 60)
            .with_reputation(0.5);
        let bid2 = JobBid::new(job_id.clone(), "peer_2".into(), to_micro(8.0), 150, 60)
            .with_reputation(0.8);
        let bid3 = JobBid::new(job_id.clone(), "peer_3".into(), to_micro(5.0), 300, 60)
            .with_reputation(0.3);

        let bids = vec![bid1, bid2, bid3];

        // Best bid should consider price, latency, and reputation
        let best = select_best_bid(&bids, Some(250)).unwrap();
        assert_eq!(best.bidder_id, "peer_2"); // Best balance of factors
    }

    #[test]
    fn test_bid_id_uniqueness() {
        let id1 = BidId::new();
        let id2 = BidId::new();
        assert_ne!(id1, id2);
    }
}
