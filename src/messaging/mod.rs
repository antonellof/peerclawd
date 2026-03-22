//! Messaging system for multi-platform communication.
//!
//! This module provides an abstraction layer for sending and receiving messages
//! across different platforms (Telegram, Discord, Slack, CLI, webhooks, etc.).
//!
//! # P2P Integration
//!
//! Messages can be routed through the P2P network:
//! - Agents can receive messages from remote peers
//! - Responses can be forwarded across the network
//! - Platform channels can be loaded as WASM plugins
//!
//! # Trust Model
//!
//! - Local channels: Full access to all tools
//! - Network channels: Restricted based on peer reputation
//! - WASM channels: Sandboxed with explicit capabilities

mod channel;
mod registry;
pub mod platforms;

pub use channel::{
    Channel, ChannelMessage, ChannelConfig, MessageDirection,
    MessageType, Attachment, ChannelError, MessageRouting,
};
pub use registry::{ChannelRegistry, ChannelHandle};

use serde::{Deserialize, Serialize};

/// Maximum message size (1 MB).
pub const MAX_MESSAGE_SIZE: usize = 1024 * 1024;

/// Maximum attachment size (10 MB).
pub const MAX_ATTACHMENT_SIZE: usize = 10 * 1024 * 1024;

/// Unique identifier for a message.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub String);

impl MessageId {
    /// Generate a new message ID.
    pub fn new() -> Self {
        Self(format!("msg_{}", uuid::Uuid::new_v4().to_string().replace("-", "")))
    }

    /// Create from an external platform ID.
    pub fn from_external(platform: &str, id: &str) -> Self {
        Self(format!("{}:{}", platform, id))
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a channel instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub String);

impl ChannelId {
    /// Generate a new channel ID.
    pub fn new(platform: &str) -> Self {
        Self(format!("{}_{}", platform, &uuid::Uuid::new_v4().to_string().replace("-", "")[..12]))
    }

    /// Create from platform and instance identifier.
    pub fn from_parts(platform: &str, instance: &str) -> Self {
        Self(format!("{}:{}", platform, instance))
    }

    /// Get the platform name.
    pub fn platform(&self) -> &str {
        self.0.split(':').next().unwrap_or(&self.0)
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// User identity from a messaging platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelUser {
    /// Platform-specific user ID.
    pub id: String,
    /// Display name.
    pub name: Option<String>,
    /// Username/handle.
    pub username: Option<String>,
    /// Whether this user is the bot/agent itself.
    pub is_self: bool,
    /// Trust level (affects tool access).
    pub trust_level: UserTrust,
    /// Associated peer ID (if from P2P network).
    pub peer_id: Option<String>,
}

impl ChannelUser {
    /// Create a new channel user.
    pub fn new(id: String) -> Self {
        Self {
            id,
            name: None,
            username: None,
            is_self: false,
            trust_level: UserTrust::Unknown,
            peer_id: None,
        }
    }

    /// Create a user from a P2P peer.
    pub fn from_peer(peer_id: String) -> Self {
        Self {
            id: peer_id.clone(),
            name: None,
            username: None,
            is_self: false,
            trust_level: UserTrust::Verified,
            peer_id: Some(peer_id),
        }
    }
}

/// Trust level for channel users.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserTrust {
    /// Unknown user, minimal permissions.
    #[default]
    Unknown = 0,
    /// Verified identity on platform.
    Verified = 1,
    /// Trusted user (added to allow list).
    Trusted = 2,
    /// Owner/admin of the agent.
    Owner = 3,
}

/// Conversation/thread context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    /// Unique conversation ID.
    pub id: String,
    /// Channel this conversation belongs to.
    pub channel_id: ChannelId,
    /// Participants in the conversation.
    pub participants: Vec<ChannelUser>,
    /// Message history (recent messages only, for context).
    pub recent_messages: Vec<ChannelMessage>,
    /// Metadata (thread topic, etc.).
    pub metadata: serde_json::Value,
}

impl Conversation {
    /// Create a new conversation.
    pub fn new(id: String, channel_id: ChannelId) -> Self {
        Self {
            id,
            channel_id,
            participants: Vec::new(),
            recent_messages: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Add a message to the conversation history.
    pub fn add_message(&mut self, message: ChannelMessage) {
        self.recent_messages.push(message);
        // Keep only last 50 messages
        if self.recent_messages.len() > 50 {
            self.recent_messages.drain(0..self.recent_messages.len() - 50);
        }
    }
}

/// Platform type enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    /// Command-line REPL.
    Repl,
    /// HTTP webhook endpoint.
    Webhook,
    /// WebSocket connection.
    WebSocket,
    /// Telegram bot.
    Telegram,
    /// Discord bot.
    Discord,
    /// Slack app.
    Slack,
    /// Matrix client.
    Matrix,
    /// P2P network (direct peer messaging).
    P2p,
    /// WASM-based custom platform.
    Wasm,
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Repl => write!(f, "repl"),
            Platform::Webhook => write!(f, "webhook"),
            Platform::WebSocket => write!(f, "websocket"),
            Platform::Telegram => write!(f, "telegram"),
            Platform::Discord => write!(f, "discord"),
            Platform::Slack => write!(f, "slack"),
            Platform::Matrix => write!(f, "matrix"),
            Platform::P2p => write!(f, "p2p"),
            Platform::Wasm => write!(f, "wasm"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id() {
        let id = MessageId::new();
        assert!(id.0.starts_with("msg_"));

        let external = MessageId::from_external("telegram", "12345");
        assert_eq!(external.0, "telegram:12345");
    }

    #[test]
    fn test_channel_id() {
        let id = ChannelId::new("telegram");
        assert!(id.0.starts_with("telegram_"));

        let parts = ChannelId::from_parts("discord", "guild123");
        assert_eq!(parts.platform(), "discord");
    }

    #[test]
    fn test_user_trust_ordering() {
        assert!(UserTrust::Unknown < UserTrust::Verified);
        assert!(UserTrust::Verified < UserTrust::Trusted);
        assert!(UserTrust::Trusted < UserTrust::Owner);
    }
}
