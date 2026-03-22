//! Channel trait and message types.
//!
//! Defines the core abstraction for messaging channels.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{ChannelId, ChannelUser, MessageId, Platform};

/// Error type for channel operations.
#[derive(Error, Debug)]
pub enum ChannelError {
    #[error("Channel not connected")]
    NotConnected,

    #[error("Connection failed: {0}")]
    ConnectionFailed(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Message too large: {size} bytes (max {max})")]
    MessageTooLarge { size: usize, max: usize },

    #[error("Rate limited: retry after {retry_after_secs} seconds")]
    RateLimited { retry_after_secs: u64 },

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid message format: {0}")]
    InvalidFormat(String),

    #[error("Platform error: {0}")]
    PlatformError(String),

    #[error("Timeout after {0} seconds")]
    Timeout(u64),

    #[error("Channel closed")]
    Closed,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Direction of a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageDirection {
    /// Incoming message from user to agent.
    Incoming,
    /// Outgoing message from agent to user.
    Outgoing,
}

/// Type of message content.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    /// Plain text message.
    #[default]
    Text,
    /// Markdown formatted message.
    Markdown,
    /// Rich formatted message (platform-specific).
    Rich,
    /// Image message.
    Image,
    /// File/document message.
    File,
    /// Audio message.
    Audio,
    /// Video message.
    Video,
    /// Reaction/emoji.
    Reaction,
    /// System message (join, leave, etc.).
    System,
    /// Command (slash command, etc.).
    Command,
}

/// File attachment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    /// Unique identifier.
    pub id: String,
    /// File name.
    pub name: String,
    /// MIME type.
    pub mime_type: String,
    /// File size in bytes.
    pub size: usize,
    /// URL to fetch the file (if available).
    pub url: Option<String>,
    /// Raw data (for small files or after fetch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<u8>>,
}

impl Attachment {
    /// Create a new attachment from data.
    pub fn from_data(name: String, mime_type: String, data: Vec<u8>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            mime_type,
            size: data.len(),
            url: None,
            data: Some(data),
        }
    }

    /// Create a new attachment from URL.
    pub fn from_url(name: String, mime_type: String, size: usize, url: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            mime_type,
            size,
            url: Some(url),
            data: None,
        }
    }
}

/// A message sent or received through a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    /// Unique message ID.
    pub id: MessageId,
    /// Channel this message belongs to.
    pub channel_id: ChannelId,
    /// Conversation/thread ID.
    pub conversation_id: Option<String>,
    /// Message direction.
    pub direction: MessageDirection,
    /// Message type.
    pub message_type: MessageType,
    /// Sender.
    pub sender: ChannelUser,
    /// Text content.
    pub content: String,
    /// File attachments.
    pub attachments: Vec<Attachment>,
    /// Reply-to message ID (if this is a reply).
    pub reply_to: Option<MessageId>,
    /// Timestamp.
    pub timestamp: DateTime<Utc>,
    /// Platform-specific metadata.
    pub metadata: serde_json::Value,
    /// P2P routing info (for network-routed messages).
    pub routing: Option<MessageRouting>,
}

impl ChannelMessage {
    /// Create a new text message.
    pub fn text(
        channel_id: ChannelId,
        sender: ChannelUser,
        content: String,
        direction: MessageDirection,
    ) -> Self {
        Self {
            id: MessageId::new(),
            channel_id,
            conversation_id: None,
            direction,
            message_type: MessageType::Text,
            sender,
            content,
            attachments: Vec::new(),
            reply_to: None,
            timestamp: Utc::now(),
            metadata: serde_json::Value::Null,
            routing: None,
        }
    }

    /// Create an outgoing response message.
    pub fn response(channel_id: ChannelId, content: String) -> Self {
        Self {
            id: MessageId::new(),
            channel_id,
            conversation_id: None,
            direction: MessageDirection::Outgoing,
            message_type: MessageType::Text,
            sender: ChannelUser {
                id: "agent".to_string(),
                name: Some("Agent".to_string()),
                username: None,
                is_self: true,
                trust_level: super::UserTrust::Owner,
                peer_id: None,
            },
            content,
            attachments: Vec::new(),
            reply_to: None,
            timestamp: Utc::now(),
            metadata: serde_json::Value::Null,
            routing: None,
        }
    }

    /// Set the conversation ID.
    pub fn with_conversation(mut self, conversation_id: String) -> Self {
        self.conversation_id = Some(conversation_id);
        self
    }

    /// Set as a reply to another message.
    pub fn with_reply_to(mut self, message_id: MessageId) -> Self {
        self.reply_to = Some(message_id);
        self
    }

    /// Add an attachment.
    pub fn with_attachment(mut self, attachment: Attachment) -> Self {
        self.attachments.push(attachment);
        self
    }

    /// Set message type.
    pub fn with_type(mut self, message_type: MessageType) -> Self {
        self.message_type = message_type;
        self
    }

    /// Set P2P routing info.
    pub fn with_routing(mut self, routing: MessageRouting) -> Self {
        self.routing = Some(routing);
        self
    }
}

/// P2P routing information for cross-network messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRouting {
    /// Original sender peer ID.
    pub source_peer: String,
    /// Destination peer ID (if targeted).
    pub target_peer: Option<String>,
    /// Intermediate hops.
    pub hops: Vec<String>,
    /// Time-to-live (max hops).
    pub ttl: u8,
    /// Message signature.
    pub signature: Option<String>,
}

impl MessageRouting {
    /// Create new routing info for a local message.
    pub fn local(peer_id: String) -> Self {
        Self {
            source_peer: peer_id,
            target_peer: None,
            hops: Vec::new(),
            ttl: 5,
            signature: None,
        }
    }

    /// Create routing info for a targeted message.
    pub fn to_peer(source_peer: String, target_peer: String) -> Self {
        Self {
            source_peer,
            target_peer: Some(target_peer),
            hops: Vec::new(),
            ttl: 5,
            signature: None,
        }
    }
}

/// Configuration for a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    /// Platform type.
    pub platform: Platform,
    /// Channel instance name.
    pub name: String,
    /// Whether the channel is enabled.
    pub enabled: bool,
    /// Platform-specific configuration.
    pub settings: serde_json::Value,
    /// Allowed user IDs (empty = allow all).
    pub allowed_users: Vec<String>,
    /// Maximum message rate (messages per minute, 0 = unlimited).
    pub rate_limit: u32,
    /// Whether to log messages.
    pub logging: bool,
    /// P2P settings.
    pub p2p: P2pChannelConfig,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            platform: Platform::Repl,
            name: "default".to_string(),
            enabled: true,
            settings: serde_json::Value::Null,
            allowed_users: Vec::new(),
            rate_limit: 0,
            logging: false,
            p2p: P2pChannelConfig::default(),
        }
    }
}

/// P2P-specific channel configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct P2pChannelConfig {
    /// Allow messages from network peers.
    pub accept_network_messages: bool,
    /// Forward messages to network.
    pub broadcast_messages: bool,
    /// Required reputation score for network messages.
    pub min_reputation: i32,
    /// Price per message (in micro-PCLAW).
    pub price_per_message: u64,
}

/// The Channel trait defines the interface for messaging platforms.
#[async_trait]
pub trait Channel: Send + Sync {
    /// Get the channel ID.
    fn id(&self) -> &ChannelId;

    /// Get the platform type.
    fn platform(&self) -> Platform;

    /// Get the channel configuration.
    fn config(&self) -> &ChannelConfig;

    /// Check if the channel is connected.
    fn is_connected(&self) -> bool;

    /// Connect to the platform.
    async fn connect(&mut self) -> Result<(), ChannelError>;

    /// Disconnect from the platform.
    async fn disconnect(&mut self) -> Result<(), ChannelError>;

    /// Send a message.
    async fn send(&self, message: ChannelMessage) -> Result<MessageId, ChannelError>;

    /// Receive the next message (blocking).
    async fn receive(&mut self) -> Result<ChannelMessage, ChannelError>;

    /// Try to receive a message without blocking.
    async fn try_receive(&mut self) -> Result<Option<ChannelMessage>, ChannelError>;

    /// Edit a previously sent message.
    async fn edit(&self, message_id: &MessageId, new_content: String) -> Result<(), ChannelError> {
        let _ = (message_id, new_content);
        Err(ChannelError::PlatformError("Edit not supported".into()))
    }

    /// Delete a message.
    async fn delete(&self, message_id: &MessageId) -> Result<(), ChannelError> {
        let _ = message_id;
        Err(ChannelError::PlatformError("Delete not supported".into()))
    }

    /// React to a message.
    async fn react(&self, message_id: &MessageId, reaction: &str) -> Result<(), ChannelError> {
        let _ = (message_id, reaction);
        Err(ChannelError::PlatformError("Reactions not supported".into()))
    }

    /// Get conversation history.
    async fn get_history(
        &self,
        conversation_id: &str,
        limit: usize,
    ) -> Result<Vec<ChannelMessage>, ChannelError> {
        let _ = (conversation_id, limit);
        Ok(Vec::new())
    }

    /// Typing indicator.
    async fn start_typing(&self, conversation_id: &str) -> Result<(), ChannelError> {
        let _ = conversation_id;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_message_creation() {
        let channel_id = super::super::ChannelId::new("test");
        let sender = ChannelUser::new("user123".to_string());

        let msg = ChannelMessage::text(
            channel_id.clone(),
            sender,
            "Hello, world!".to_string(),
            MessageDirection::Incoming,
        );

        assert_eq!(msg.content, "Hello, world!");
        assert_eq!(msg.direction, MessageDirection::Incoming);
        assert_eq!(msg.message_type, MessageType::Text);
    }

    #[test]
    fn test_attachment_from_data() {
        let data = b"Hello, world!".to_vec();
        let attachment = Attachment::from_data(
            "test.txt".to_string(),
            "text/plain".to_string(),
            data.clone(),
        );

        assert_eq!(attachment.name, "test.txt");
        assert_eq!(attachment.size, data.len());
        assert!(attachment.data.is_some());
    }

    #[test]
    fn test_message_routing() {
        let routing = MessageRouting::to_peer("peer_a".to_string(), "peer_b".to_string());

        assert_eq!(routing.source_peer, "peer_a");
        assert_eq!(routing.target_peer, Some("peer_b".to_string()));
        assert_eq!(routing.ttl, 5);
    }
}
