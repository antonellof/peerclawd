//! Transaction types for the wallet ledger.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

use super::escrow::EscrowId;

/// Unique identifier for a transaction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionId(pub String);

impl TransactionId {
    /// Generate a new random transaction ID.
    pub fn new() -> Self {
        Self(format!("tx_{}", Uuid::new_v4().to_string().replace("-", "")))
    }
}

impl Default for TransactionId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TransactionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for TransactionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Type of transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionType {
    /// Tokens credited to the wallet (earning).
    Credit { reason: String },
    /// Tokens debited from the wallet (spending).
    Debit { reason: String },
    /// Tokens moved to escrow for a job.
    EscrowCreated { escrow_id: EscrowId },
    /// Escrow released to worker (job succeeded).
    EscrowReleased { escrow_id: EscrowId },
    /// Escrow refunded to sender (job failed).
    EscrowRefunded { escrow_id: EscrowId },
    /// Tokens staked as provider bond.
    Stake,
    /// Tokens unstaked (withdrawn from bond).
    Unstake,
    /// Tokens slashed (penalty for bad behavior).
    Slash { reason: String },
    /// Transfer to another wallet.
    TransferOut { recipient: String },
    /// Transfer received from another wallet.
    TransferIn { sender: String },
}

impl fmt::Display for TransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionType::Credit { reason } => write!(f, "Credit ({})", reason),
            TransactionType::Debit { reason } => write!(f, "Debit ({})", reason),
            TransactionType::EscrowCreated { escrow_id } => {
                write!(f, "Escrow Created ({})", escrow_id)
            }
            TransactionType::EscrowReleased { escrow_id } => {
                write!(f, "Escrow Released ({})", escrow_id)
            }
            TransactionType::EscrowRefunded { escrow_id } => {
                write!(f, "Escrow Refunded ({})", escrow_id)
            }
            TransactionType::Stake => write!(f, "Stake"),
            TransactionType::Unstake => write!(f, "Unstake"),
            TransactionType::Slash { reason } => write!(f, "Slash ({})", reason),
            TransactionType::TransferOut { recipient } => write!(f, "Transfer to {}", recipient),
            TransactionType::TransferIn { sender } => write!(f, "Transfer from {}", sender),
        }
    }
}

/// A transaction record in the wallet ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique identifier.
    pub id: TransactionId,
    /// Type of transaction.
    pub tx_type: TransactionType,
    /// Amount in μPCLAW.
    pub amount: u64,
    /// When the transaction occurred.
    pub timestamp: DateTime<Utc>,
    /// Whether this transaction added or removed tokens from available balance.
    pub direction: TransactionDirection,
}

/// Direction of the transaction relative to available balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionDirection {
    /// Tokens added to available balance.
    In,
    /// Tokens removed from available balance.
    Out,
    /// Internal movement (e.g., stake/unstake, escrow).
    Internal,
}

impl fmt::Display for TransactionDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransactionDirection::In => write!(f, "+"),
            TransactionDirection::Out => write!(f, "-"),
            TransactionDirection::Internal => write!(f, "~"),
        }
    }
}

impl Transaction {
    /// Create a new credit transaction.
    pub fn new_credit(amount: u64, reason: String) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::Credit { reason },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::In,
        }
    }

    /// Create a new debit transaction.
    pub fn new_debit(amount: u64, reason: String) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::Debit { reason },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::Out,
        }
    }

    /// Create a new escrow creation transaction.
    pub fn new_escrow_created(amount: u64, escrow_id: EscrowId) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::EscrowCreated { escrow_id },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::Out,
        }
    }

    /// Create a new escrow released transaction.
    pub fn new_escrow_released(amount: u64, escrow_id: EscrowId) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::EscrowReleased { escrow_id },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::Internal,
        }
    }

    /// Create a new escrow refunded transaction.
    pub fn new_escrow_refunded(amount: u64, escrow_id: EscrowId) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::EscrowRefunded { escrow_id },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::In,
        }
    }

    /// Create a new stake transaction.
    pub fn new_stake(amount: u64) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::Stake,
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::Internal,
        }
    }

    /// Create a new unstake transaction.
    pub fn new_unstake(amount: u64) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::Unstake,
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::Internal,
        }
    }

    /// Create a new slash transaction.
    pub fn new_slash(amount: u64, reason: String) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::Slash { reason },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::Out,
        }
    }

    /// Create a new transfer out transaction.
    pub fn new_transfer_out(amount: u64, recipient: String) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::TransferOut { recipient },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::Out,
        }
    }

    /// Create a new transfer in transaction.
    pub fn new_transfer_in(amount: u64, sender: String) -> Self {
        Self {
            id: TransactionId::new(),
            tx_type: TransactionType::TransferIn { sender },
            amount,
            timestamp: Utc::now(),
            direction: TransactionDirection::In,
        }
    }

    /// Get amount in PCLAW (float).
    pub fn amount_pclaw(&self) -> f64 {
        super::from_micro(self.amount)
    }

    /// Format for display in CLI.
    pub fn display_line(&self) -> String {
        let direction_char = match self.direction {
            TransactionDirection::In => "+",
            TransactionDirection::Out => "-",
            TransactionDirection::Internal => "~",
        };

        format!(
            "{} {} {:.6} PCLAW  {}  {}",
            self.timestamp.format("%Y-%m-%d %H:%M"),
            direction_char,
            self.amount_pclaw(),
            self.tx_type,
            self.id
        )
    }
}

impl fmt::Display for Transaction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_line())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_credit() {
        let tx = Transaction::new_credit(1_000_000, "test reward".to_string());
        assert_eq!(tx.amount, 1_000_000);
        assert_eq!(tx.direction, TransactionDirection::In);
        assert!(matches!(tx.tx_type, TransactionType::Credit { .. }));
    }

    #[test]
    fn test_transaction_debit() {
        let tx = Transaction::new_debit(500_000, "inference".to_string());
        assert_eq!(tx.amount, 500_000);
        assert_eq!(tx.direction, TransactionDirection::Out);
        assert!(matches!(tx.tx_type, TransactionType::Debit { .. }));
    }

    #[test]
    fn test_transaction_display() {
        let tx = Transaction::new_credit(1_000_000, "test".to_string());
        let display = tx.display_line();
        assert!(display.contains("+"));
        assert!(display.contains("PCLAW"));
        assert!(display.contains("Credit"));
    }

    #[test]
    fn test_transaction_id_uniqueness() {
        let id1 = TransactionId::new();
        let id2 = TransactionId::new();
        assert_ne!(id1, id2);
    }
}
