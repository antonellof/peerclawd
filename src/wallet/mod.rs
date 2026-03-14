//! Token wallet implementation for PeerClaw'd.
//!
//! Provides local token accounting, transaction management, and balance tracking.
//! The PCLAW token has 6 decimal places (1 PCLAW = 1_000_000 μPCLAW).

mod balance;
mod escrow;
mod transaction;

pub use balance::{Balance, BalanceSnapshot};
pub use escrow::{Escrow, EscrowId, EscrowStatus};
pub use transaction::{Transaction, TransactionId, TransactionType};

use crate::db::Database;
use crate::identity::NodeIdentity;
use ed25519_dalek::Signature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// Token precision: 6 decimal places.
/// 1 PCLAW = 1_000_000 μPCLAW (micro-PCLAW)
pub const TOKEN_DECIMALS: u32 = 6;
pub const MICRO_PCLAW: u64 = 1_000_000;

/// Convert PCLAW to μPCLAW
pub fn to_micro(pclaw: f64) -> u64 {
    (pclaw * MICRO_PCLAW as f64) as u64
}

/// Convert μPCLAW to PCLAW
pub fn from_micro(micro: u64) -> f64 {
    micro as f64 / MICRO_PCLAW as f64
}

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("Insufficient balance: have {available} μPCLAW, need {required} μPCLAW")]
    InsufficientBalance { available: u64, required: u64 },

    #[error("Escrow not found: {0}")]
    EscrowNotFound(EscrowId),

    #[error("Escrow already resolved: {0}")]
    EscrowAlreadyResolved(EscrowId),

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Transaction not found: {0}")]
    TransactionNotFound(TransactionId),

    #[error("Invalid amount: {0}")]
    InvalidAmount(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Spending limit exceeded: {limit_type} limit is {limit} μPCLAW")]
    SpendingLimitExceeded { limit_type: String, limit: u64 },
}

/// Wallet configuration for spending controls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Maximum tokens to spend per single transaction (in μPCLAW)
    pub max_spend_per_tx: u64,
    /// Maximum tokens to spend per hour (in μPCLAW)
    pub max_spend_per_hour: u64,
    /// Maximum tokens to spend per day (in μPCLAW)
    pub max_spend_per_day: u64,
    /// Reserve balance to always maintain (in μPCLAW)
    pub reserve_balance: u64,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            max_spend_per_tx: to_micro(100.0),      // 100 PCLAW per tx
            max_spend_per_hour: to_micro(500.0),    // 500 PCLAW per hour
            max_spend_per_day: to_micro(2000.0),    // 2000 PCLAW per day
            reserve_balance: to_micro(100.0),       // Keep 100 PCLAW reserve
        }
    }
}

/// The main wallet manager.
pub struct Wallet {
    identity: Arc<NodeIdentity>,
    state: RwLock<WalletState>,
    config: WalletConfig,
    database: Database,
}

/// Persistent wallet state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletState {
    /// Available balance in μPCLAW
    pub available: u64,
    /// Amount currently held in escrow in μPCLAW
    pub in_escrow: u64,
    /// Amount staked as provider bond in μPCLAW
    pub staked: u64,
    /// Active escrows
    pub escrows: HashMap<EscrowId, Escrow>,
    /// Transaction history (recent, older ones pruned)
    pub transactions: Vec<Transaction>,
    /// Spending tracker: (hour_timestamp, amount_spent)
    pub hourly_spend: HashMap<u64, u64>,
    /// Spending tracker: (day_timestamp, amount_spent)
    pub daily_spend: HashMap<u64, u64>,
}

impl Default for WalletState {
    fn default() -> Self {
        Self {
            available: 0,
            in_escrow: 0,
            staked: 0,
            escrows: HashMap::new(),
            transactions: Vec::new(),
            hourly_spend: HashMap::new(),
            daily_spend: HashMap::new(),
        }
    }
}

impl Wallet {
    /// Create a new wallet for the given identity.
    pub fn new(
        identity: Arc<NodeIdentity>,
        config: WalletConfig,
        database: Database,
    ) -> Result<Self, WalletError> {
        // Try to load existing state from database
        let state = Self::load_state(&database)?;

        Ok(Self {
            identity,
            state: RwLock::new(state),
            config,
            database,
        })
    }

    /// Load wallet state from database.
    fn load_state(database: &Database) -> Result<WalletState, WalletError> {
        match database.get_setting::<WalletState>("wallet_state") {
            Ok(Some(state)) => Ok(state),
            Ok(None) => Ok(WalletState::default()),
            Err(e) => Err(WalletError::Database(e.to_string())),
        }
    }

    /// Save wallet state to database.
    async fn save_state(&self) -> Result<(), WalletError> {
        let state = self.state.read().await;
        self.database
            .store_setting("wallet_state", &*state)
            .map_err(|e| WalletError::Database(e.to_string()))
    }

    /// Get the wallet's address (peer ID as string).
    pub fn address(&self) -> String {
        self.identity.peer_id().to_string()
    }

    /// Get a snapshot of the current balance.
    pub async fn balance(&self) -> BalanceSnapshot {
        let state = self.state.read().await;
        BalanceSnapshot {
            available: state.available,
            in_escrow: state.in_escrow,
            staked: state.staked,
            total: state.available + state.in_escrow + state.staked,
        }
    }

    /// Credit tokens to the wallet (e.g., from earning resources).
    pub async fn credit(&self, amount: u64, reason: &str) -> Result<Transaction, WalletError> {
        if amount == 0 {
            return Err(WalletError::InvalidAmount("Amount must be > 0".into()));
        }

        let mut state = self.state.write().await;
        state.available += amount;

        let tx = Transaction::new_credit(amount, reason.to_string());
        state.transactions.push(tx.clone());

        // Trim old transactions (keep last 1000)
        let len = state.transactions.len();
        if len > 1000 {
            state.transactions.drain(0..len - 1000);
        }

        drop(state);
        self.save_state().await?;

        Ok(tx)
    }

    /// Debit tokens from the wallet.
    pub async fn debit(&self, amount: u64, reason: &str) -> Result<Transaction, WalletError> {
        self.check_spending_limits(amount).await?;

        let mut state = self.state.write().await;

        // Check available balance (respecting reserve)
        let effective_available = state.available.saturating_sub(self.config.reserve_balance);
        if amount > effective_available {
            return Err(WalletError::InsufficientBalance {
                available: effective_available,
                required: amount,
            });
        }

        state.available -= amount;

        // Update spending trackers
        let now = chrono::Utc::now().timestamp() as u64;
        let hour_key = now / 3600;
        let day_key = now / 86400;

        *state.hourly_spend.entry(hour_key).or_insert(0) += amount;
        *state.daily_spend.entry(day_key).or_insert(0) += amount;

        // Clean old spend tracking entries
        let old_hour = hour_key.saturating_sub(24);
        let old_day = day_key.saturating_sub(30);
        state.hourly_spend.retain(|&k, _| k > old_hour);
        state.daily_spend.retain(|&k, _| k > old_day);

        let tx = Transaction::new_debit(amount, reason.to_string());
        state.transactions.push(tx.clone());

        drop(state);
        self.save_state().await?;

        Ok(tx)
    }

    /// Check spending limits before a transaction.
    async fn check_spending_limits(&self, amount: u64) -> Result<(), WalletError> {
        // Check per-transaction limit
        if amount > self.config.max_spend_per_tx {
            return Err(WalletError::SpendingLimitExceeded {
                limit_type: "per-transaction".into(),
                limit: self.config.max_spend_per_tx,
            });
        }

        let state = self.state.read().await;
        let now = chrono::Utc::now().timestamp() as u64;
        let hour_key = now / 3600;
        let day_key = now / 86400;

        // Check hourly limit
        let hourly_spent = state.hourly_spend.get(&hour_key).copied().unwrap_or(0);
        if hourly_spent + amount > self.config.max_spend_per_hour {
            return Err(WalletError::SpendingLimitExceeded {
                limit_type: "hourly".into(),
                limit: self.config.max_spend_per_hour,
            });
        }

        // Check daily limit
        let daily_spent = state.daily_spend.get(&day_key).copied().unwrap_or(0);
        if daily_spent + amount > self.config.max_spend_per_day {
            return Err(WalletError::SpendingLimitExceeded {
                limit_type: "daily".into(),
                limit: self.config.max_spend_per_day,
            });
        }

        Ok(())
    }

    /// Create an escrow for a job payment.
    pub async fn create_escrow(
        &self,
        amount: u64,
        recipient: String,
        job_id: String,
        timeout_secs: u64,
    ) -> Result<Escrow, WalletError> {
        self.check_spending_limits(amount).await?;

        let mut state = self.state.write().await;

        // Check available balance
        let effective_available = state.available.saturating_sub(self.config.reserve_balance);
        if amount > effective_available {
            return Err(WalletError::InsufficientBalance {
                available: effective_available,
                required: amount,
            });
        }

        // Move funds to escrow
        state.available -= amount;
        state.in_escrow += amount;

        let escrow = Escrow::new(amount, recipient, job_id, timeout_secs);
        state.escrows.insert(escrow.id.clone(), escrow.clone());

        let tx = Transaction::new_escrow_created(amount, escrow.id.clone());
        state.transactions.push(tx);

        drop(state);
        self.save_state().await?;

        Ok(escrow)
    }

    /// Release escrow to the recipient (job completed successfully).
    pub async fn release_escrow(&self, escrow_id: &EscrowId) -> Result<Transaction, WalletError> {
        let mut state = self.state.write().await;

        // Get amount and check status first
        let amount = {
            let escrow = state
                .escrows
                .get(escrow_id)
                .ok_or_else(|| WalletError::EscrowNotFound(escrow_id.clone()))?;

            if escrow.status != EscrowStatus::Active {
                return Err(WalletError::EscrowAlreadyResolved(escrow_id.clone()));
            }
            escrow.amount
        };

        // Now mutate
        if let Some(escrow) = state.escrows.get_mut(escrow_id) {
            escrow.status = EscrowStatus::Released;
        }
        state.in_escrow -= amount;

        let tx = Transaction::new_escrow_released(amount, escrow_id.clone());
        state.transactions.push(tx.clone());

        drop(state);
        self.save_state().await?;

        Ok(tx)
    }

    /// Refund escrow back to sender (job failed or timed out).
    pub async fn refund_escrow(&self, escrow_id: &EscrowId) -> Result<Transaction, WalletError> {
        let mut state = self.state.write().await;

        // Get amount and check status first
        let amount = {
            let escrow = state
                .escrows
                .get(escrow_id)
                .ok_or_else(|| WalletError::EscrowNotFound(escrow_id.clone()))?;

            if escrow.status != EscrowStatus::Active {
                return Err(WalletError::EscrowAlreadyResolved(escrow_id.clone()));
            }
            escrow.amount
        };

        // Now mutate
        if let Some(escrow) = state.escrows.get_mut(escrow_id) {
            escrow.status = EscrowStatus::Refunded;
        }
        state.in_escrow -= amount;
        state.available += amount;

        let tx = Transaction::new_escrow_refunded(amount, escrow_id.clone());
        state.transactions.push(tx.clone());

        drop(state);
        self.save_state().await?;

        Ok(tx)
    }

    /// Stake tokens as resource provider bond.
    pub async fn stake(&self, amount: u64) -> Result<Transaction, WalletError> {
        let mut state = self.state.write().await;

        if amount > state.available {
            return Err(WalletError::InsufficientBalance {
                available: state.available,
                required: amount,
            });
        }

        state.available -= amount;
        state.staked += amount;

        let tx = Transaction::new_stake(amount);
        state.transactions.push(tx.clone());

        drop(state);
        self.save_state().await?;

        Ok(tx)
    }

    /// Unstake tokens (withdraw from provider bond).
    pub async fn unstake(&self, amount: u64) -> Result<Transaction, WalletError> {
        let mut state = self.state.write().await;

        if amount > state.staked {
            return Err(WalletError::InsufficientBalance {
                available: state.staked,
                required: amount,
            });
        }

        state.staked -= amount;
        state.available += amount;

        let tx = Transaction::new_unstake(amount);
        state.transactions.push(tx.clone());

        drop(state);
        self.save_state().await?;

        Ok(tx)
    }

    /// Get recent transactions.
    pub async fn transactions(&self, limit: usize) -> Vec<Transaction> {
        let state = self.state.read().await;
        let len = state.transactions.len();
        let start = len.saturating_sub(limit);
        state.transactions[start..].to_vec()
    }

    /// Get active escrows.
    pub async fn active_escrows(&self) -> Vec<Escrow> {
        let state = self.state.read().await;
        state
            .escrows
            .values()
            .filter(|e| e.status == EscrowStatus::Active)
            .cloned()
            .collect()
    }

    /// Sign a message with the wallet's private key.
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.identity.sign(message)
    }

    /// Verify a signature against the wallet's public key.
    pub fn verify(&self, message: &[u8], signature: &Signature) -> bool {
        self.identity.verify(message, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_test_wallet() -> (Wallet, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.redb")).unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let config = WalletConfig::default();
        let wallet = Wallet::new(identity, config, db).unwrap();
        (wallet, dir)
    }

    #[tokio::test]
    async fn test_credit_and_balance() {
        let (wallet, _dir) = setup_test_wallet();

        wallet.credit(to_micro(100.0), "test credit").await.unwrap();

        let balance = wallet.balance().await;
        assert_eq!(balance.available, to_micro(100.0));
        assert_eq!(balance.total, to_micro(100.0));
    }

    #[tokio::test]
    async fn test_debit_insufficient_balance() {
        let (wallet, _dir) = setup_test_wallet();

        let result = wallet.debit(to_micro(100.0), "test debit").await;
        assert!(matches!(result, Err(WalletError::InsufficientBalance { .. })));
    }

    #[tokio::test]
    async fn test_escrow_lifecycle() {
        let (wallet, _dir) = setup_test_wallet();

        // Credit some tokens
        wallet.credit(to_micro(200.0), "initial").await.unwrap();

        // Create escrow
        let escrow = wallet
            .create_escrow(
                to_micro(50.0),
                "recipient".to_string(),
                "job-1".to_string(),
                3600,
            )
            .await
            .unwrap();

        let balance = wallet.balance().await;
        assert_eq!(balance.available, to_micro(150.0));
        assert_eq!(balance.in_escrow, to_micro(50.0));

        // Release escrow
        wallet.release_escrow(&escrow.id).await.unwrap();

        let balance = wallet.balance().await;
        assert_eq!(balance.available, to_micro(150.0));
        assert_eq!(balance.in_escrow, 0);
    }

    #[tokio::test]
    async fn test_stake_and_unstake() {
        let (wallet, _dir) = setup_test_wallet();

        wallet.credit(to_micro(1000.0), "initial").await.unwrap();
        wallet.stake(to_micro(500.0)).await.unwrap();

        let balance = wallet.balance().await;
        assert_eq!(balance.available, to_micro(500.0));
        assert_eq!(balance.staked, to_micro(500.0));

        wallet.unstake(to_micro(200.0)).await.unwrap();

        let balance = wallet.balance().await;
        assert_eq!(balance.available, to_micro(700.0));
        assert_eq!(balance.staked, to_micro(300.0));
    }
}
