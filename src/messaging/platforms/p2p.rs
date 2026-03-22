//! P2P channel for direct peer-to-peer messaging.
//!
//! Enables agents to communicate directly through the P2P network,
//! with optional token payments for message processing.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::messaging::{
    Channel, ChannelConfig, ChannelError, ChannelId, ChannelMessage, ChannelUser,
    MessageDirection, MessageId, MessageRouting, Platform, UserTrust,
};

/// P2P channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct P2pConfig {
    /// Whether to accept messages from unknown peers.
    pub accept_unknown_peers: bool,
    /// Minimum reputation score to accept messages.
    pub min_reputation: i32,
    /// Price per incoming message (micro-PCLAW).
    pub price_per_message: u64,
    /// Maximum pending messages.
    pub max_pending: usize,
    /// Message TTL (time-to-live in hops).
    pub message_ttl: u8,
    /// Allowed peer IDs (empty = allow all).
    pub allowed_peers: Vec<String>,
    /// Blocked peer IDs.
    pub blocked_peers: Vec<String>,
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self {
            accept_unknown_peers: false,
            min_reputation: 0,
            price_per_message: 0,
            max_pending: 1000,
            message_ttl: 5,
            allowed_peers: Vec::new(),
            blocked_peers: Vec::new(),
        }
    }
}

/// P2P channel for network messaging.
pub struct P2pChannel {
    /// Channel ID.
    id: ChannelId,
    /// Local peer ID.
    local_peer_id: String,
    /// Configuration.
    config: ChannelConfig,
    /// P2P-specific config.
    p2p_config: P2pConfig,
    /// Whether connected.
    connected: bool,
    /// Incoming message queue.
    incoming_rx: Arc<RwLock<mpsc::Receiver<ChannelMessage>>>,
    /// Sender for incoming messages (used by network layer).
    incoming_tx: Arc<mpsc::Sender<ChannelMessage>>,
    /// Outgoing message queue (to be sent by network layer).
    outgoing_tx: Arc<mpsc::Sender<ChannelMessage>>,
    /// Outgoing receiver (for network layer to consume).
    outgoing_rx: Arc<RwLock<mpsc::Receiver<ChannelMessage>>>,
    /// Statistics.
    stats: Arc<RwLock<P2pChannelStats>>,
}

/// P2P channel statistics.
#[derive(Debug, Clone, Default)]
pub struct P2pChannelStats {
    /// Messages received from network.
    pub messages_received: u64,
    /// Messages sent to network.
    pub messages_sent: u64,
    /// Messages rejected (blocked, no payment, etc.).
    pub messages_rejected: u64,
    /// Unique peers interacted with.
    pub unique_peers: std::collections::HashSet<String>,
    /// Total tokens earned from messages.
    pub tokens_earned: u64,
}

impl P2pChannel {
    /// Create a new P2P channel.
    pub fn new(config: ChannelConfig) -> Self {
        Self::with_peer_id(config, "unknown".to_string())
    }

    /// Create a new P2P channel with peer ID.
    pub fn with_peer_id(config: ChannelConfig, local_peer_id: String) -> Self {
        let id = ChannelId::from_parts("p2p", &config.name);
        let p2p_config: P2pConfig = if config.settings.is_null() {
            P2pConfig::default()
        } else {
            serde_json::from_value(config.settings.clone()).unwrap_or_default()
        };

        let (in_tx, in_rx) = mpsc::channel(p2p_config.max_pending);
        let (out_tx, out_rx) = mpsc::channel(p2p_config.max_pending);

        Self {
            id,
            local_peer_id,
            config,
            p2p_config,
            connected: false,
            incoming_rx: Arc::new(RwLock::new(in_rx)),
            incoming_tx: Arc::new(in_tx),
            outgoing_tx: Arc::new(out_tx),
            outgoing_rx: Arc::new(RwLock::new(out_rx)),
            stats: Arc::new(RwLock::new(P2pChannelStats::default())),
        }
    }

    /// Get the incoming message sender (for network layer).
    pub fn incoming_sender(&self) -> Arc<mpsc::Sender<ChannelMessage>> {
        self.incoming_tx.clone()
    }

    /// Get the outgoing message receiver (for network layer).
    pub fn outgoing_receiver(&self) -> Arc<RwLock<mpsc::Receiver<ChannelMessage>>> {
        self.outgoing_rx.clone()
    }

    /// Get channel statistics.
    pub async fn stats(&self) -> P2pChannelStats {
        self.stats.read().await.clone()
    }

    /// Check if a peer is allowed.
    pub fn is_peer_allowed(&self, peer_id: &str) -> bool {
        // Check blocked list first
        if self.p2p_config.blocked_peers.contains(&peer_id.to_string()) {
            return false;
        }

        // If allowed list is empty, allow all (unless accept_unknown_peers is false)
        if self.p2p_config.allowed_peers.is_empty() {
            return self.p2p_config.accept_unknown_peers;
        }

        // Check allowed list
        self.p2p_config.allowed_peers.contains(&peer_id.to_string())
    }

    /// Get the price for processing a message.
    pub fn message_price(&self) -> u64 {
        self.p2p_config.price_per_message
    }

    /// Process an incoming P2P message.
    pub async fn receive_from_network(
        &self,
        sender_peer_id: String,
        content: String,
        routing: Option<MessageRouting>,
    ) -> Result<(), ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        // Check if peer is allowed
        if !self.is_peer_allowed(&sender_peer_id) {
            let mut stats = self.stats.write().await;
            stats.messages_rejected += 1;
            return Err(ChannelError::PermissionDenied(format!(
                "Peer {} is not allowed",
                sender_peer_id
            )));
        }

        let sender = ChannelUser::from_peer(sender_peer_id.clone());

        let mut message = ChannelMessage::text(
            self.id.clone(),
            sender,
            content,
            MessageDirection::Incoming,
        );

        // Add routing info
        message.routing = routing.or_else(|| Some(MessageRouting::local(sender_peer_id.clone())));

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.messages_received += 1;
            stats.unique_peers.insert(sender_peer_id);
        }

        // Queue the message
        self.incoming_tx
            .send(message)
            .await
            .map_err(|_| ChannelError::Closed)
    }

    /// Send a message to a specific peer.
    pub async fn send_to_peer(
        &self,
        target_peer_id: String,
        content: String,
    ) -> Result<MessageId, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let sender = ChannelUser {
            id: self.local_peer_id.clone(),
            name: None,
            username: None,
            is_self: true,
            trust_level: UserTrust::Owner,
            peer_id: Some(self.local_peer_id.clone()),
        };

        let mut message = ChannelMessage::text(
            self.id.clone(),
            sender,
            content,
            MessageDirection::Outgoing,
        );

        // Set routing
        message.routing = Some(MessageRouting::to_peer(
            self.local_peer_id.clone(),
            target_peer_id,
        ));

        let message_id = message.id.clone();

        // Queue for network layer
        self.outgoing_tx
            .send(message)
            .await
            .map_err(|_| ChannelError::Closed)?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.messages_sent += 1;
        }

        Ok(message_id)
    }
}

#[async_trait]
impl Channel for P2pChannel {
    fn id(&self) -> &ChannelId {
        &self.id
    }

    fn platform(&self) -> Platform {
        Platform::P2p
    }

    fn config(&self) -> &ChannelConfig {
        &self.config
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn connect(&mut self) -> Result<(), ChannelError> {
        if self.connected {
            return Ok(());
        }

        self.connected = true;
        tracing::info!(
            channel_id = %self.id,
            peer_id = %self.local_peer_id,
            "P2P channel connected"
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), ChannelError> {
        self.connected = false;
        tracing::info!(channel_id = %self.id, "P2P channel disconnected");
        Ok(())
    }

    async fn send(&self, message: ChannelMessage) -> Result<MessageId, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let message_id = message.id.clone();

        // Queue for network layer
        self.outgoing_tx
            .send(message)
            .await
            .map_err(|_| ChannelError::Closed)?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.messages_sent += 1;
        }

        Ok(message_id)
    }

    async fn receive(&mut self) -> Result<ChannelMessage, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let mut rx = self.incoming_rx.write().await;
        rx.recv().await.ok_or(ChannelError::Closed)
    }

    async fn try_receive(&mut self) -> Result<Option<ChannelMessage>, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let mut rx = self.incoming_rx.write().await;
        match rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(ChannelError::Closed),
        }
    }
}

/// P2P message envelope for network transmission.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct P2pMessageEnvelope {
    /// Message ID.
    pub id: String,
    /// Sender peer ID.
    pub sender: String,
    /// Target peer ID (if directed).
    pub target: Option<String>,
    /// Message content.
    pub content: String,
    /// Timestamp (Unix epoch seconds).
    pub timestamp: i64,
    /// TTL (hops remaining).
    pub ttl: u8,
    /// Routing hops.
    pub hops: Vec<String>,
    /// Ed25519 signature.
    pub signature: String,
    /// Payment info (if required).
    pub payment: Option<P2pPaymentInfo>,
}

/// Payment information for P2P messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct P2pPaymentInfo {
    /// Payment channel ID.
    pub channel_id: String,
    /// Amount in micro-PCLAW.
    pub amount: u64,
    /// Payment nonce.
    pub nonce: u64,
    /// Signed payment update.
    pub signature: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_p2p_channel_creation() {
        let config = ChannelConfig {
            platform: Platform::P2p,
            name: "test".to_string(),
            ..Default::default()
        };

        let channel = P2pChannel::with_peer_id(config, "peer_abc123".to_string());
        assert_eq!(channel.platform(), Platform::P2p);
        assert!(!channel.is_connected());
    }

    #[test]
    fn test_peer_allowed_empty_list() {
        let config = ChannelConfig {
            platform: Platform::P2p,
            name: "test".to_string(),
            settings: serde_json::json!({
                "accept_unknown_peers": true
            }),
            ..Default::default()
        };

        let channel = P2pChannel::new(config);
        assert!(channel.is_peer_allowed("any_peer"));
    }

    #[test]
    fn test_peer_blocked() {
        let config = ChannelConfig {
            platform: Platform::P2p,
            name: "test".to_string(),
            settings: serde_json::json!({
                "accept_unknown_peers": true,
                "blocked_peers": ["bad_peer"]
            }),
            ..Default::default()
        };

        let channel = P2pChannel::new(config);
        assert!(!channel.is_peer_allowed("bad_peer"));
        assert!(channel.is_peer_allowed("good_peer"));
    }

    #[test]
    fn test_peer_allowed_list() {
        let config = ChannelConfig {
            platform: Platform::P2p,
            name: "test".to_string(),
            settings: serde_json::json!({
                "allowed_peers": ["peer_a", "peer_b"]
            }),
            ..Default::default()
        };

        let channel = P2pChannel::new(config);
        assert!(channel.is_peer_allowed("peer_a"));
        assert!(channel.is_peer_allowed("peer_b"));
        assert!(!channel.is_peer_allowed("peer_c"));
    }
}
