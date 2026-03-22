//! Webhook channel for HTTP-based communication.
//!
//! Receives messages via HTTP POST and sends responses via webhooks.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

use crate::messaging::{
    Channel, ChannelConfig, ChannelError, ChannelId, ChannelMessage, ChannelUser,
    MessageDirection, MessageId, Platform, UserTrust,
};

/// Webhook channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Incoming webhook path (where we receive messages).
    pub incoming_path: String,
    /// Outgoing webhook URL (where we send responses).
    pub outgoing_url: Option<String>,
    /// Secret for verifying incoming webhooks.
    pub secret: Option<String>,
    /// HTTP timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            incoming_path: "/webhook".to_string(),
            outgoing_url: None,
            secret: None,
            timeout_secs: 30,
        }
    }
}

/// Webhook channel for HTTP-based messaging.
pub struct WebhookChannel {
    /// Channel ID.
    id: ChannelId,
    /// Configuration.
    config: ChannelConfig,
    /// Webhook-specific config.
    webhook_config: WebhookConfig,
    /// Whether connected.
    connected: bool,
    /// Incoming message queue.
    incoming: Arc<RwLock<mpsc::Receiver<ChannelMessage>>>,
    /// Sender for incoming messages (used by webhook handler).
    incoming_tx: Arc<mpsc::Sender<ChannelMessage>>,
    /// HTTP client for outgoing webhooks.
    client: Option<reqwest::Client>,
}

impl WebhookChannel {
    /// Create a new webhook channel.
    pub fn new(config: ChannelConfig) -> Self {
        let id = ChannelId::from_parts("webhook", &config.name);
        let webhook_config: WebhookConfig = if config.settings.is_null() {
            WebhookConfig::default()
        } else {
            serde_json::from_value(config.settings.clone()).unwrap_or_default()
        };

        let (tx, rx) = mpsc::channel(256);

        Self {
            id,
            config,
            webhook_config,
            connected: false,
            incoming: Arc::new(RwLock::new(rx)),
            incoming_tx: Arc::new(tx),
            client: None,
        }
    }

    /// Get the incoming message sender (for the webhook HTTP handler).
    pub fn incoming_sender(&self) -> Arc<mpsc::Sender<ChannelMessage>> {
        self.incoming_tx.clone()
    }

    /// Get the incoming webhook path.
    pub fn incoming_path(&self) -> &str {
        &self.webhook_config.incoming_path
    }

    /// Verify incoming webhook signature.
    pub fn verify_signature(&self, payload: &[u8], signature: &str) -> bool {
        match &self.webhook_config.secret {
            Some(secret) => {
                // Simple HMAC verification (would use proper HMAC-SHA256 in production)
                let expected = blake3::keyed_hash(
                    blake3::hash(secret.as_bytes()).as_bytes(),
                    payload,
                );
                signature == expected.to_hex().as_str()
            }
            None => true, // No secret configured, accept all
        }
    }

    /// Parse an incoming webhook payload into a message.
    pub fn parse_payload(&self, payload: &str) -> Result<ChannelMessage, ChannelError> {
        // Try to parse as JSON
        let value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|e| ChannelError::InvalidFormat(e.to_string()))?;

        // Extract message content
        let content = value.get("message")
            .or_else(|| value.get("text"))
            .or_else(|| value.get("content"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChannelError::InvalidFormat("Missing message content".into()))?
            .to_string();

        // Extract sender info
        let sender_id = value.get("user_id")
            .or_else(|| value.get("sender"))
            .and_then(|v| v.as_str())
            .unwrap_or("webhook_user")
            .to_string();

        let sender_name = value.get("user_name")
            .or_else(|| value.get("name"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let sender = ChannelUser {
            id: sender_id,
            name: sender_name,
            username: None,
            is_self: false,
            trust_level: UserTrust::Verified,
            peer_id: None,
        };

        Ok(ChannelMessage::text(
            self.id.clone(),
            sender,
            content,
            MessageDirection::Incoming,
        ))
    }
}

#[async_trait]
impl Channel for WebhookChannel {
    fn id(&self) -> &ChannelId {
        &self.id
    }

    fn platform(&self) -> Platform {
        Platform::Webhook
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

        // Create HTTP client for outgoing webhooks
        self.client = Some(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(self.webhook_config.timeout_secs))
                .build()
                .map_err(|e| ChannelError::ConnectionFailed(e.to_string()))?,
        );

        self.connected = true;
        tracing::info!(
            channel_id = %self.id,
            incoming_path = %self.webhook_config.incoming_path,
            "Webhook channel connected"
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), ChannelError> {
        self.client = None;
        self.connected = false;
        tracing::info!(channel_id = %self.id, "Webhook channel disconnected");
        Ok(())
    }

    async fn send(&self, message: ChannelMessage) -> Result<MessageId, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        // If we have an outgoing URL, POST the message
        if let Some(url) = &self.webhook_config.outgoing_url {
            let client = self.client.as_ref().ok_or(ChannelError::NotConnected)?;

            let payload = serde_json::json!({
                "message_id": message.id.0,
                "content": message.content,
                "timestamp": message.timestamp.to_rfc3339(),
                "channel_id": self.id.0,
            });

            let response = client
                .post(url)
                .json(&payload)
                .send()
                .await
                .map_err(|e| ChannelError::PlatformError(e.to_string()))?;

            if !response.status().is_success() {
                return Err(ChannelError::PlatformError(format!(
                    "Webhook returned status {}",
                    response.status()
                )));
            }
        }

        Ok(message.id)
    }

    async fn receive(&mut self) -> Result<ChannelMessage, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let mut rx = self.incoming.write().await;
        rx.recv().await.ok_or(ChannelError::Closed)
    }

    async fn try_receive(&mut self) -> Result<Option<ChannelMessage>, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        let mut rx = self.incoming.write().await;
        match rx.try_recv() {
            Ok(msg) => Ok(Some(msg)),
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => Err(ChannelError::Closed),
        }
    }
}

/// Incoming webhook payload (generic format).
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct WebhookPayload {
    /// Message content.
    pub message: String,
    /// Optional user ID.
    pub user_id: Option<String>,
    /// Optional user name.
    pub user_name: Option<String>,
    /// Optional conversation ID.
    pub conversation_id: Option<String>,
    /// Optional metadata.
    pub metadata: Option<serde_json::Value>,
}

/// Outgoing webhook response.
#[derive(Debug, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct WebhookResponse {
    /// Response message ID.
    pub message_id: String,
    /// Response content.
    pub content: String,
    /// Timestamp.
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_channel_creation() {
        let config = ChannelConfig {
            platform: Platform::Webhook,
            name: "test".to_string(),
            ..Default::default()
        };

        let channel = WebhookChannel::new(config);
        assert_eq!(channel.platform(), Platform::Webhook);
        assert!(!channel.is_connected());
    }

    #[test]
    fn test_parse_payload() {
        let config = ChannelConfig {
            platform: Platform::Webhook,
            name: "test".to_string(),
            ..Default::default()
        };

        let channel = WebhookChannel::new(config);

        let payload = r#"{"message": "Hello, world!", "user_id": "user123"}"#;
        let message = channel.parse_payload(payload).unwrap();

        assert_eq!(message.content, "Hello, world!");
        assert_eq!(message.sender.id, "user123");
    }

    #[test]
    fn test_verify_signature_no_secret() {
        let config = ChannelConfig::default();
        let channel = WebhookChannel::new(config);

        // Should accept anything when no secret is set
        assert!(channel.verify_signature(b"test", "any_signature"));
    }
}
