//! Channel registry for managing multiple messaging channels.
//!
//! Supports local and WASM-based channels with P2P distribution.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use super::{
    Channel, ChannelConfig, ChannelError, ChannelId, ChannelMessage,
    MessageId, Platform,
};

/// Handle to a registered channel.
pub struct ChannelHandle {
    /// Channel instance.
    channel: Arc<RwLock<Box<dyn Channel>>>,
    /// Configuration.
    config: ChannelConfig,
    /// Statistics.
    stats: ChannelStats,
}

impl ChannelHandle {
    /// Get the channel ID.
    pub fn id(&self) -> ChannelId {
        ChannelId::from_parts(&self.config.platform.to_string(), &self.config.name)
    }

    /// Get the configuration.
    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    /// Get channel statistics.
    pub fn stats(&self) -> &ChannelStats {
        &self.stats
    }

    /// Send a message through this channel.
    pub async fn send(&self, message: ChannelMessage) -> Result<MessageId, ChannelError> {
        let channel = self.channel.read().await;
        channel.send(message).await
    }

    /// Check if connected.
    pub async fn is_connected(&self) -> bool {
        let channel = self.channel.read().await;
        channel.is_connected()
    }
}

/// Channel statistics.
#[derive(Debug, Clone, Default)]
pub struct ChannelStats {
    /// Total messages sent.
    pub messages_sent: u64,
    /// Total messages received.
    pub messages_received: u64,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Failed send attempts.
    pub send_failures: u64,
    /// Connection count.
    pub connections: u64,
    /// Last message timestamp.
    pub last_message: Option<chrono::DateTime<chrono::Utc>>,
}

/// Message handler callback type.
type MessageHandler = Box<dyn Fn(ChannelMessage) + Send + Sync>;

/// Registry for managing multiple channels.
pub struct ChannelRegistry {
    /// Local peer ID.
    local_peer_id: String,
    /// Registered channels.
    channels: RwLock<HashMap<ChannelId, Arc<ChannelHandle>>>,
    /// Channel configurations (pending).
    configs: RwLock<HashMap<String, ChannelConfig>>,
    /// Global message callback.
    message_handler: RwLock<Option<MessageHandler>>,
    /// Network channel discovery.
    network_channels: RwLock<HashMap<String, NetworkChannelInfo>>,
}

/// Information about a channel available on the network.
#[derive(Debug, Clone)]
pub struct NetworkChannelInfo {
    /// Provider peer ID.
    pub peer_id: String,
    /// Channel ID.
    pub channel_id: ChannelId,
    /// Platform type.
    pub platform: Platform,
    /// Whether the channel accepts network messages.
    pub accepts_messages: bool,
    /// Price per message (micro-PCLAW).
    pub price: u64,
    /// Last seen timestamp.
    pub last_seen: chrono::DateTime<chrono::Utc>,
}

impl ChannelRegistry {
    /// Create a new channel registry.
    pub fn new(local_peer_id: String) -> Self {
        Self {
            local_peer_id,
            channels: RwLock::new(HashMap::new()),
            configs: RwLock::new(HashMap::new()),
            message_handler: RwLock::new(None),
            network_channels: RwLock::new(HashMap::new()),
        }
    }

    /// Get the local peer ID.
    pub fn local_peer_id(&self) -> &str {
        &self.local_peer_id
    }

    /// Register a channel configuration.
    pub async fn add_config(&self, name: String, config: ChannelConfig) {
        self.configs.write().await.insert(name, config);
    }

    /// Register a channel instance.
    pub async fn register(
        &self,
        channel: Box<dyn Channel>,
    ) -> Result<ChannelId, ChannelError> {
        let id = channel.id().clone();
        let config = channel.config().clone();

        let handle = Arc::new(ChannelHandle {
            channel: Arc::new(RwLock::new(channel)),
            config,
            stats: ChannelStats::default(),
        });

        self.channels.write().await.insert(id.clone(), handle);

        tracing::info!(channel_id = %id, "Registered channel");
        Ok(id)
    }

    /// Get a channel by ID.
    pub async fn get(&self, id: &ChannelId) -> Option<Arc<ChannelHandle>> {
        self.channels.read().await.get(id).cloned()
    }

    /// List all registered channels.
    pub async fn list(&self) -> Vec<Arc<ChannelHandle>> {
        self.channels.read().await.values().cloned().collect()
    }

    /// List channels by platform.
    pub async fn list_by_platform(&self, platform: Platform) -> Vec<Arc<ChannelHandle>> {
        self.channels
            .read()
            .await
            .values()
            .filter(|h| h.config.platform == platform)
            .cloned()
            .collect()
    }

    /// Connect all registered channels.
    pub async fn connect_all(&self) -> Vec<(ChannelId, Result<(), ChannelError>)> {
        let channels = self.channels.read().await;
        let mut results = Vec::new();

        for (id, handle) in channels.iter() {
            let mut channel = handle.channel.write().await;
            let result = channel.connect().await;
            results.push((id.clone(), result));
        }

        results
    }

    /// Disconnect all channels.
    pub async fn disconnect_all(&self) {
        let channels = self.channels.read().await;

        for handle in channels.values() {
            let mut channel = handle.channel.write().await;
            let _ = channel.disconnect().await;
        }
    }

    /// Set a global message handler.
    pub async fn set_message_handler<F>(&self, handler: F)
    where
        F: Fn(ChannelMessage) + Send + Sync + 'static,
    {
        *self.message_handler.write().await = Some(Box::new(handler));
    }

    /// Dispatch a received message.
    pub async fn dispatch_message(&self, message: ChannelMessage) {
        if let Some(handler) = self.message_handler.read().await.as_ref() {
            handler(message);
        }
    }

    /// Broadcast a message to all channels.
    pub async fn broadcast(&self, content: String) -> Vec<(ChannelId, Result<MessageId, ChannelError>)> {
        let channels = self.channels.read().await;
        let mut results = Vec::new();

        for (id, handle) in channels.iter() {
            if !handle.config.enabled {
                continue;
            }

            let message = ChannelMessage::response(id.clone(), content.clone());
            let result = handle.send(message).await;
            results.push((id.clone(), result));
        }

        results
    }

    /// Register a network channel from peer announcement.
    pub async fn register_network_channel(&self, info: NetworkChannelInfo) {
        let key = format!("{}:{}", info.peer_id, info.channel_id);
        self.network_channels.write().await.insert(key, info);
    }

    /// Get network channels that accept messages.
    pub async fn get_network_channels(&self) -> Vec<NetworkChannelInfo> {
        self.network_channels
            .read()
            .await
            .values()
            .filter(|c| c.accepts_messages)
            .cloned()
            .collect()
    }

    /// Send a message to a network peer's channel.
    pub async fn send_to_network(
        &self,
        peer_id: &str,
        channel_id: &ChannelId,
        message: ChannelMessage,
    ) -> Result<MessageId, ChannelError> {
        // This would typically go through the P2P network
        // For now, return an error indicating it needs network implementation
        let _ = (peer_id, channel_id, message);
        Err(ChannelError::PlatformError(
            "Network message routing not yet implemented".into(),
        ))
    }

    /// Remove a channel.
    pub async fn remove(&self, id: &ChannelId) -> bool {
        let mut channels = self.channels.write().await;
        if let Some(handle) = channels.remove(id) {
            let mut channel = handle.channel.write().await;
            let _ = channel.disconnect().await;
            true
        } else {
            false
        }
    }
}

/// Builder for channel configurations.
#[allow(dead_code)]
pub struct ChannelConfigBuilder {
    config: ChannelConfig,
}

#[allow(dead_code)]
impl ChannelConfigBuilder {
    /// Create a new builder for the given platform.
    pub fn new(platform: Platform, name: &str) -> Self {
        Self {
            config: ChannelConfig {
                platform,
                name: name.to_string(),
                ..Default::default()
            },
        }
    }

    /// Enable or disable the channel.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }

    /// Set platform-specific settings.
    pub fn settings(mut self, settings: serde_json::Value) -> Self {
        self.config.settings = settings;
        self
    }

    /// Set allowed users.
    pub fn allowed_users(mut self, users: Vec<String>) -> Self {
        self.config.allowed_users = users;
        self
    }

    /// Set rate limit.
    pub fn rate_limit(mut self, rate: u32) -> Self {
        self.config.rate_limit = rate;
        self
    }

    /// Enable logging.
    pub fn logging(mut self, enabled: bool) -> Self {
        self.config.logging = enabled;
        self
    }

    /// Configure P2P settings.
    pub fn accept_network_messages(mut self, accept: bool) -> Self {
        self.config.p2p.accept_network_messages = accept;
        self
    }

    /// Configure P2P broadcast.
    pub fn broadcast_messages(mut self, broadcast: bool) -> Self {
        self.config.p2p.broadcast_messages = broadcast;
        self
    }

    /// Set message price.
    pub fn price_per_message(mut self, price: u64) -> Self {
        self.config.p2p.price_per_message = price;
        self
    }

    /// Build the configuration.
    pub fn build(self) -> ChannelConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder() {
        let config = ChannelConfigBuilder::new(Platform::Telegram, "my_bot")
            .enabled(true)
            .rate_limit(60)
            .accept_network_messages(true)
            .price_per_message(100)
            .build();

        assert_eq!(config.platform, Platform::Telegram);
        assert_eq!(config.name, "my_bot");
        assert!(config.enabled);
        assert_eq!(config.rate_limit, 60);
        assert!(config.p2p.accept_network_messages);
        assert_eq!(config.p2p.price_per_message, 100);
    }

    #[tokio::test]
    async fn test_registry_creation() {
        let registry = ChannelRegistry::new("test-peer".to_string());
        let channels = registry.list().await;
        assert!(channels.is_empty());
    }
}
