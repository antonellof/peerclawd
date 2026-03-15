//! Payment channels for high-frequency micropayments.
//!
//! Implements a simplified Lightning-style payment channel system
//! for efficient off-chain token transfers between peers.

mod state;

pub use state::{ChannelState, ChannelUpdate, SignedUpdate};

use crate::identity::NodeIdentity;
use crate::wallet::{from_micro, Wallet, WalletError};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("Channel not found: {0}")]
    NotFound(ChannelId),

    #[error("Channel already exists with peer: {0}")]
    AlreadyExists(String),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Insufficient channel balance: have {available}, need {required}")]
    InsufficientBalance { available: u64, required: u64 },

    #[error("Invalid signature")]
    InvalidSignature,

    #[error("Nonce mismatch: expected {expected}, got {got}")]
    NonceMismatch { expected: u64, got: u64 },

    #[error("Channel expired")]
    Expired,

    #[error("Wallet error: {0}")]
    Wallet(#[from] WalletError),
}

/// Unique identifier for a payment channel.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub String);

impl ChannelId {
    /// Generate a new channel ID.
    pub fn new() -> Self {
        Self(format!("chan_{}", Uuid::new_v4().to_string().replace("-", "")))
    }

    /// Create a deterministic channel ID from two peer IDs.
    /// The ID is the same regardless of which peer initiates.
    pub fn from_peers(peer_a: &str, peer_b: &str) -> Self {
        let (first, second) = if peer_a < peer_b {
            (peer_a, peer_b)
        } else {
            (peer_b, peer_a)
        };
        let hash = blake3::hash(format!("{}:{}", first, second).as_bytes());
        Self(format!("chan_{}", &hash.to_hex()[..32]))
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Status of a payment channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelStatus {
    /// Channel is being opened, awaiting peer confirmation
    Opening,
    /// Channel is open and operational
    Open,
    /// Channel is being closed cooperatively
    Closing,
    /// Channel has been closed
    Closed,
    /// Channel was force-closed due to dispute
    Disputed,
}

impl std::fmt::Display for ChannelStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChannelStatus::Opening => write!(f, "Opening"),
            ChannelStatus::Open => write!(f, "Open"),
            ChannelStatus::Closing => write!(f, "Closing"),
            ChannelStatus::Closed => write!(f, "Closed"),
            ChannelStatus::Disputed => write!(f, "Disputed"),
        }
    }
}

/// A payment channel between two peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentChannel {
    /// Unique identifier
    pub id: ChannelId,
    /// Local peer ID
    pub local_peer: String,
    /// Remote peer ID
    pub remote_peer: String,
    /// Total capacity locked in channel (in μPCLAW)
    pub capacity: u64,
    /// Local balance (our side of the channel)
    pub local_balance: u64,
    /// Remote balance (their side of the channel)
    pub remote_balance: u64,
    /// Current state nonce
    pub nonce: u64,
    /// Channel status
    pub status: ChannelStatus,
    /// When the channel was opened
    pub opened_at: DateTime<Utc>,
    /// When the channel expires (must be closed by this time)
    pub expires_at: DateTime<Utc>,
    /// Latest signed state from remote peer
    pub latest_remote_signature: Option<String>,
    /// Number of updates made
    pub update_count: u64,
    /// Total amount sent through channel
    pub total_sent: u64,
    /// Total amount received through channel
    pub total_received: u64,
}

impl PaymentChannel {
    /// Create a new channel (local peer is the funder).
    pub fn new(
        local_peer: String,
        remote_peer: String,
        capacity: u64,
        duration_hours: u64,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: ChannelId::from_peers(&local_peer, &remote_peer),
            local_peer,
            remote_peer,
            capacity,
            local_balance: capacity, // Funder starts with full balance
            remote_balance: 0,
            nonce: 0,
            status: ChannelStatus::Opening,
            opened_at: now,
            expires_at: now + chrono::Duration::hours(duration_hours as i64),
            latest_remote_signature: None,
            update_count: 0,
            total_sent: 0,
            total_received: 0,
        }
    }

    /// Check if channel is expired.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if channel is operational.
    pub fn is_operational(&self) -> bool {
        self.status == ChannelStatus::Open && !self.is_expired()
    }

    /// Get time remaining until expiration.
    pub fn time_remaining(&self) -> chrono::Duration {
        self.expires_at - Utc::now()
    }

    /// Create a payment (transfer from local to remote).
    pub fn create_payment(&mut self, amount: u64) -> Result<ChannelUpdate, ChannelError> {
        if !self.is_operational() {
            return Err(ChannelError::InvalidState(
                "Channel is not operational".into(),
            ));
        }

        if amount > self.local_balance {
            return Err(ChannelError::InsufficientBalance {
                available: self.local_balance,
                required: amount,
            });
        }

        self.local_balance -= amount;
        self.remote_balance += amount;
        self.nonce += 1;
        self.update_count += 1;
        self.total_sent += amount;

        Ok(ChannelUpdate {
            channel_id: self.id.clone(),
            nonce: self.nonce,
            local_balance: self.local_balance,
            remote_balance: self.remote_balance,
            timestamp: Utc::now(),
        })
    }

    /// Apply a payment received from remote peer.
    pub fn receive_payment(&mut self, update: &ChannelUpdate) -> Result<(), ChannelError> {
        if !self.is_operational() {
            return Err(ChannelError::InvalidState(
                "Channel is not operational".into(),
            ));
        }

        if update.nonce != self.nonce + 1 {
            return Err(ChannelError::NonceMismatch {
                expected: self.nonce + 1,
                got: update.nonce,
            });
        }

        // Verify balances are valid
        if update.local_balance + update.remote_balance != self.capacity {
            return Err(ChannelError::InvalidState(
                "Balance mismatch in update".into(),
            ));
        }

        // Calculate amount received (their local_balance is our remote_balance view)
        let amount_received = update.local_balance.saturating_sub(self.remote_balance);

        self.local_balance = update.remote_balance;
        self.remote_balance = update.local_balance;
        self.nonce = update.nonce;
        self.update_count += 1;
        self.total_received += amount_received;

        Ok(())
    }

    /// Get capacity in PCLAW.
    pub fn capacity_pclaw(&self) -> f64 {
        from_micro(self.capacity)
    }

    /// Get local balance in PCLAW.
    pub fn local_balance_pclaw(&self) -> f64 {
        from_micro(self.local_balance)
    }

    /// Get remote balance in PCLAW.
    pub fn remote_balance_pclaw(&self) -> f64 {
        from_micro(self.remote_balance)
    }
}

impl std::fmt::Display for PaymentChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Channel[{}]: {} ↔ {} ({:.6}/{:.6} PCLAW) [{}]",
            self.id,
            &self.local_peer[..8.min(self.local_peer.len())],
            &self.remote_peer[..8.min(self.remote_peer.len())],
            self.local_balance_pclaw(),
            self.capacity_pclaw(),
            self.status
        )
    }
}

/// Manager for payment channels.
pub struct ChannelManager {
    /// Local identity for signing
    identity: Arc<NodeIdentity>,
    /// Reference to wallet for on-chain settlement
    wallet: Arc<Wallet>,
    /// Active channels by ID
    channels: RwLock<HashMap<ChannelId, PaymentChannel>>,
    /// Channels by remote peer ID
    by_peer: RwLock<HashMap<String, ChannelId>>,
}

impl ChannelManager {
    /// Create a new channel manager.
    pub fn new(identity: Arc<NodeIdentity>, wallet: Arc<Wallet>) -> Self {
        Self {
            identity,
            wallet,
            channels: RwLock::new(HashMap::new()),
            by_peer: RwLock::new(HashMap::new()),
        }
    }

    /// Open a new channel with a peer.
    pub async fn open_channel(
        &self,
        remote_peer: String,
        capacity: u64,
        duration_hours: u64,
    ) -> Result<PaymentChannel, ChannelError> {
        let local_peer = self.identity.peer_id().to_string();

        // Check if channel already exists
        {
            let by_peer = self.by_peer.read().await;
            if by_peer.contains_key(&remote_peer) {
                return Err(ChannelError::AlreadyExists(remote_peer));
            }
        }

        // Lock funds from wallet
        let _escrow = self.wallet.create_escrow(
            capacity,
            remote_peer.clone(),
            format!("channel_{}", remote_peer),
            duration_hours * 3600,
        ).await?;

        // Create channel
        let channel = PaymentChannel::new(local_peer, remote_peer.clone(), capacity, duration_hours);
        let channel_id = channel.id.clone();

        // Store channel
        {
            let mut channels = self.channels.write().await;
            channels.insert(channel_id.clone(), channel.clone());
        }
        {
            let mut by_peer = self.by_peer.write().await;
            by_peer.insert(remote_peer, channel_id);
        }

        Ok(channel)
    }

    /// Get a channel by ID.
    pub async fn get_channel(&self, id: &ChannelId) -> Option<PaymentChannel> {
        self.channels.read().await.get(id).cloned()
    }

    /// Get channel with a specific peer.
    pub async fn get_channel_with_peer(&self, peer_id: &str) -> Option<PaymentChannel> {
        let channel_id = self.by_peer.read().await.get(peer_id).cloned()?;
        self.get_channel(&channel_id).await
    }

    /// Confirm channel opening (called when remote peer accepts).
    pub async fn confirm_channel(&self, channel_id: &ChannelId) -> Result<(), ChannelError> {
        let mut channels = self.channels.write().await;
        if let Some(channel) = channels.get_mut(channel_id) {
            if channel.status != ChannelStatus::Opening {
                return Err(ChannelError::InvalidState(
                    "Channel is not in Opening state".into(),
                ));
            }
            channel.status = ChannelStatus::Open;
            Ok(())
        } else {
            Err(ChannelError::NotFound(channel_id.clone()))
        }
    }

    /// Make a payment through a channel.
    pub async fn pay(
        &self,
        channel_id: &ChannelId,
        amount: u64,
    ) -> Result<SignedUpdate, ChannelError> {
        let mut channels = self.channels.write().await;
        let channel = channels.get_mut(channel_id)
            .ok_or_else(|| ChannelError::NotFound(channel_id.clone()))?;

        let update = channel.create_payment(amount)?;

        // Sign the update
        let signed = update.sign(&self.identity);

        Ok(signed)
    }

    /// Pay a peer (opens channel if needed, pays through existing otherwise).
    pub async fn pay_peer(
        &self,
        peer_id: &str,
        amount: u64,
    ) -> Result<SignedUpdate, ChannelError> {
        // Try to find existing channel
        if let Some(channel_id) = self.by_peer.read().await.get(peer_id).cloned() {
            return self.pay(&channel_id, amount).await;
        }

        // No channel exists - for now, require explicit channel opening
        Err(ChannelError::NotFound(ChannelId::from_peers(
            &self.identity.peer_id().to_string(),
            peer_id,
        )))
    }

    /// Receive a payment update from remote peer.
    pub async fn receive_payment(
        &self,
        signed_update: &SignedUpdate,
    ) -> Result<(), ChannelError> {
        // Verify signature
        // TODO: Get remote peer's public key and verify

        let mut channels = self.channels.write().await;
        let channel = channels.get_mut(&signed_update.update.channel_id)
            .ok_or_else(|| ChannelError::NotFound(signed_update.update.channel_id.clone()))?;

        channel.receive_payment(&signed_update.update)?;
        channel.latest_remote_signature = Some(signed_update.signature.clone());

        Ok(())
    }

    /// Close a channel cooperatively.
    pub async fn close_channel(&self, channel_id: &ChannelId) -> Result<(), ChannelError> {
        let mut channels = self.channels.write().await;
        let channel = channels.get_mut(channel_id)
            .ok_or_else(|| ChannelError::NotFound(channel_id.clone()))?;

        if !matches!(channel.status, ChannelStatus::Open | ChannelStatus::Opening) {
            return Err(ChannelError::InvalidState(
                "Channel cannot be closed from this state".into(),
            ));
        }

        channel.status = ChannelStatus::Closing;

        // TODO: Broadcast closing state to remote peer
        // TODO: Wait for cooperative close or timeout

        // For now, immediate close
        channel.status = ChannelStatus::Closed;

        // Remove from by_peer index
        {
            let mut by_peer = self.by_peer.write().await;
            by_peer.remove(&channel.remote_peer);
        }

        Ok(())
    }

    /// Get all active channels.
    pub async fn list_channels(&self) -> Vec<PaymentChannel> {
        self.channels.read().await.values().cloned().collect()
    }

    /// Get channel statistics.
    pub async fn stats(&self) -> ChannelStats {
        let channels = self.channels.read().await;

        let mut stats = ChannelStats::default();
        for channel in channels.values() {
            stats.total_channels += 1;
            if channel.is_operational() {
                stats.active_channels += 1;
                stats.total_capacity += channel.capacity;
                stats.total_local_balance += channel.local_balance;
            }
            stats.total_sent += channel.total_sent;
            stats.total_received += channel.total_received;
            stats.total_updates += channel.update_count;
        }

        stats
    }
}

/// Statistics for payment channels.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelStats {
    /// Total number of channels (including closed)
    pub total_channels: usize,
    /// Number of active channels
    pub active_channels: usize,
    /// Total capacity across active channels (in μPCLAW)
    pub total_capacity: u64,
    /// Total local balance across active channels
    pub total_local_balance: u64,
    /// Total amount sent through all channels
    pub total_sent: u64,
    /// Total amount received through all channels
    pub total_received: u64,
    /// Total number of channel updates
    pub total_updates: u64,
}

impl std::fmt::Display for ChannelStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Channel Statistics")?;
        writeln!(f, "──────────────────")?;
        writeln!(f, "Active channels: {}", self.active_channels)?;
        writeln!(f, "Total capacity:  {:.6} PCLAW", from_micro(self.total_capacity))?;
        writeln!(f, "Local balance:   {:.6} PCLAW", from_micro(self.total_local_balance))?;
        writeln!(f, "Total sent:      {:.6} PCLAW", from_micro(self.total_sent))?;
        writeln!(f, "Total received:  {:.6} PCLAW", from_micro(self.total_received))?;
        writeln!(f, "Total updates:   {}", self.total_updates)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::wallet::WalletConfig;
    use tempfile::tempdir;

    fn create_test_channel() -> PaymentChannel {
        PaymentChannel::new(
            "peer_local".to_string(),
            "peer_remote".to_string(),
            to_micro(100.0),
            24,
        )
    }

    #[test]
    fn test_channel_creation() {
        let channel = create_test_channel();

        assert_eq!(channel.status, ChannelStatus::Opening);
        assert_eq!(channel.local_balance, to_micro(100.0));
        assert_eq!(channel.remote_balance, 0);
        assert_eq!(channel.nonce, 0);
    }

    #[test]
    fn test_channel_payment() {
        let mut channel = create_test_channel();
        channel.status = ChannelStatus::Open;

        let update = channel.create_payment(to_micro(10.0)).unwrap();

        assert_eq!(channel.local_balance, to_micro(90.0));
        assert_eq!(channel.remote_balance, to_micro(10.0));
        assert_eq!(update.nonce, 1);
    }

    #[test]
    fn test_channel_insufficient_balance() {
        let mut channel = create_test_channel();
        channel.status = ChannelStatus::Open;

        let result = channel.create_payment(to_micro(200.0));
        assert!(matches!(result, Err(ChannelError::InsufficientBalance { .. })));
    }

    #[test]
    fn test_channel_id_deterministic() {
        let id1 = ChannelId::from_peers("alice", "bob");
        let id2 = ChannelId::from_peers("bob", "alice");
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_channel_manager() {
        let dir = tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.redb")).unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let wallet = Arc::new(
            Wallet::new(identity.clone(), WalletConfig::default(), db).unwrap()
        );

        // Credit some tokens
        wallet.credit(to_micro(1000.0), "test").await.unwrap();

        let manager = ChannelManager::new(identity, wallet);

        // Open channel
        let channel = manager.open_channel(
            "remote_peer".to_string(),
            to_micro(100.0),
            24,
        ).await.unwrap();

        assert_eq!(channel.status, ChannelStatus::Opening);

        // Confirm channel
        manager.confirm_channel(&channel.id).await.unwrap();

        let channel = manager.get_channel(&channel.id).await.unwrap();
        assert_eq!(channel.status, ChannelStatus::Open);

        // Make payment
        let signed = manager.pay(&channel.id, to_micro(10.0)).await.unwrap();
        assert_eq!(signed.update.local_balance, to_micro(90.0));
    }
}
