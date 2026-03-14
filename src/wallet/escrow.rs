//! Escrow system for job payments.
//!
//! Escrows hold tokens temporarily while a job is being executed.
//! On success, tokens are released to the worker.
//! On failure or timeout, tokens are refunded to the sender.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for an escrow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EscrowId(pub String);

impl EscrowId {
    /// Generate a new random escrow ID.
    pub fn new() -> Self {
        Self(format!("escrow_{}", Uuid::new_v4().to_string().replace("-", "")))
    }
}

impl Default for EscrowId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for EscrowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for EscrowId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl AsRef<str> for EscrowId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Status of an escrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowStatus {
    /// Escrow is active, tokens are held.
    Active,
    /// Escrow released to recipient (job succeeded).
    Released,
    /// Escrow refunded to sender (job failed or timed out).
    Refunded,
    /// Escrow expired (timed out without resolution).
    Expired,
}

impl fmt::Display for EscrowStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EscrowStatus::Active => write!(f, "Active"),
            EscrowStatus::Released => write!(f, "Released"),
            EscrowStatus::Refunded => write!(f, "Refunded"),
            EscrowStatus::Expired => write!(f, "Expired"),
        }
    }
}

/// An escrow holding tokens for a pending job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Escrow {
    /// Unique identifier.
    pub id: EscrowId,
    /// Amount held in escrow (in μPCLAW).
    pub amount: u64,
    /// Recipient's address (peer ID) who will receive on success.
    pub recipient: String,
    /// Associated job ID.
    pub job_id: String,
    /// When the escrow was created.
    pub created_at: DateTime<Utc>,
    /// When the escrow expires (auto-refund after this time).
    pub expires_at: DateTime<Utc>,
    /// Current status.
    pub status: EscrowStatus,
    /// When the escrow was resolved (released/refunded).
    pub resolved_at: Option<DateTime<Utc>>,
}

impl Escrow {
    /// Create a new active escrow.
    pub fn new(amount: u64, recipient: String, job_id: String, timeout_secs: u64) -> Self {
        let now = Utc::now();
        Self {
            id: EscrowId::new(),
            amount,
            recipient,
            job_id,
            created_at: now,
            expires_at: now + chrono::Duration::seconds(timeout_secs as i64),
            status: EscrowStatus::Active,
            resolved_at: None,
        }
    }

    /// Check if the escrow has expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if the escrow is still active (not resolved and not expired).
    pub fn is_active(&self) -> bool {
        self.status == EscrowStatus::Active && !self.is_expired()
    }

    /// Get time remaining until expiration.
    pub fn time_remaining(&self) -> chrono::Duration {
        self.expires_at - Utc::now()
    }

    /// Get amount in PCLAW (float).
    pub fn amount_pclaw(&self) -> f64 {
        super::from_micro(self.amount)
    }
}

impl PartialEq for Escrow {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Escrow {}

impl std::hash::Hash for Escrow {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_escrow_creation() {
        let escrow = Escrow::new(
            1_000_000,
            "recipient_peer_id".to_string(),
            "job_123".to_string(),
            3600,
        );

        assert!(escrow.is_active());
        assert!(!escrow.is_expired());
        assert_eq!(escrow.status, EscrowStatus::Active);
        assert_eq!(escrow.amount, 1_000_000);
    }

    #[test]
    fn test_escrow_expiration() {
        let escrow = Escrow::new(
            1_000_000,
            "recipient".to_string(),
            "job".to_string(),
            0, // Immediate expiration
        );

        // Give it a moment to expire
        sleep(Duration::from_millis(10));

        assert!(escrow.is_expired());
        assert!(!escrow.is_active());
    }

    #[test]
    fn test_escrow_id_uniqueness() {
        let id1 = EscrowId::new();
        let id2 = EscrowId::new();
        assert_ne!(id1, id2);
    }
}
