//! HTTP webhook channel for receiving external messages.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;
use tokio::sync::mpsc;

use super::{Channel, ChannelCapabilities, IncomingMessage, OutgoingResponse};

/// Webhook channel for receiving HTTP POST messages
pub struct WebhookChannel {
    name: String,
    path: String,
    secret: Option<String>,
    pending_responses: Arc<RwLock<HashMap<String, OutgoingResponse>>>,
}

impl WebhookChannel {
    /// Create a new webhook channel
    pub fn new(name: &str, path: &str) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            secret: None,
            pending_responses: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set webhook secret for signature validation
    pub fn with_secret(mut self, secret: &str) -> Self {
        self.secret = Some(secret.to_string());
        self
    }

    /// Get the webhook path
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Validate webhook signature
    pub fn validate_signature(&self, payload: &[u8], signature: &str) -> bool {
        let Some(secret) = &self.secret else {
            return true; // No secret configured, skip validation
        };

        // HMAC-SHA256 validation
        use std::io::Write;
        let mut mac = blake3::Hasher::new_keyed(
            &blake3::derive_key("webhook-signature", secret.as_bytes())
        );
        mac.write_all(payload).ok();
        let expected = mac.finalize().to_hex();

        // Compare signatures
        signature.trim_start_matches("sha256=") == expected.as_str()
    }

    /// Handle incoming webhook
    pub fn handle_webhook(
        &self,
        body: &str,
        _headers: &HashMap<String, String>,
    ) -> anyhow::Result<IncomingMessage> {
        // Parse webhook body (assuming JSON)
        let json: serde_json::Value = serde_json::from_str(body)?;

        // Extract message content
        let content = json.get("text")
            .or(json.get("content"))
            .or(json.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let user_id = json.get("user_id")
            .or(json.get("user"))
            .or(json.get("from"))
            .and_then(|v| v.as_str())
            .unwrap_or("webhook-user");

        let thread_id = json.get("thread_id")
            .or(json.get("thread"))
            .and_then(|v| v.as_str());

        let mut msg = IncomingMessage::new(&self.name, user_id, content);

        if let Some(tid) = thread_id {
            msg = msg.with_thread(tid);
        }

        // Store platform data
        msg.metadata.platform_data = Some(json);

        Ok(msg)
    }

    /// Get pending response for a message ID
    pub fn get_response(&self, message_id: &str) -> Option<OutgoingResponse> {
        let mut responses = self.pending_responses.write();
        responses.remove(message_id)
    }
}

#[async_trait]
impl Channel for WebhookChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "HTTP webhook endpoint for external integrations"
    }

    async fn start(&self, _tx: mpsc::Sender<IncomingMessage>) -> anyhow::Result<()> {
        // Webhook channels are started by the web server
        // This is a no-op
        Ok(())
    }

    async fn send(&self, response: OutgoingResponse) -> anyhow::Result<()> {
        // Store response for retrieval
        let mut responses = self.pending_responses.write();
        responses.insert(response.id.clone(), response);
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            can_send_text: true,
            can_send_attachments: true,
            can_receive_attachments: true,
            supports_threading: true,
            supports_streaming: false,
            supports_rich_format: true,
            max_message_length: 65536,
            rate_limit: Some(100),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_creation() {
        let channel = WebhookChannel::new("slack", "/webhook/slack");
        assert_eq!(channel.name(), "slack");
        assert_eq!(channel.path(), "/webhook/slack");
    }

    #[test]
    fn test_webhook_parsing() {
        let channel = WebhookChannel::new("test", "/webhook/test");

        let body = r#"{"text": "Hello!", "user_id": "user123"}"#;
        let headers = HashMap::new();

        let msg = channel.handle_webhook(body, &headers).unwrap();
        assert_eq!(msg.content, "Hello!");
        assert_eq!(msg.user_id, "user123");
    }
}
