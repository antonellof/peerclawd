//! REPL (Read-Eval-Print-Loop) channel for CLI interaction.
//!
//! Provides an interactive command-line interface for the agent.

use async_trait::async_trait;
use std::io::{self, Write};
use tokio::sync::mpsc;

use crate::messaging::{
    Channel, ChannelConfig, ChannelError, ChannelId, ChannelMessage, ChannelUser,
    MessageDirection, MessageId, Platform, UserTrust,
};

/// REPL channel for CLI interaction.
pub struct ReplChannel {
    /// Channel ID.
    id: ChannelId,
    /// Configuration.
    config: ChannelConfig,
    /// Whether connected.
    connected: bool,
    /// Message receiver (from stdin reader).
    rx: Option<mpsc::Receiver<ChannelMessage>>,
    /// Shutdown signal.
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl ReplChannel {
    /// Create a new REPL channel.
    pub fn new(config: ChannelConfig) -> Self {
        let id = ChannelId::from_parts("repl", &config.name);
        Self {
            id,
            config,
            connected: false,
            rx: None,
            shutdown_tx: None,
        }
    }

    /// Get prompt string.
    fn prompt(&self) -> &str {
        "> "
    }

    /// Print the prompt.
    fn print_prompt(&self) {
        print!("{}", self.prompt());
        let _ = io::stdout().flush();
    }
}

#[async_trait]
impl Channel for ReplChannel {
    fn id(&self) -> &ChannelId {
        &self.id
    }

    fn platform(&self) -> Platform {
        Platform::Repl
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

        // Create message channel
        let (tx, rx) = mpsc::channel(32);
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        let channel_id = self.id.clone();
        let prompt = self.prompt().to_string();

        // Spawn stdin reader task using spawn_blocking for stdin
        tokio::spawn(async move {
            print!("{}", prompt);
            let _ = io::stdout().flush();

            loop {
                // Use spawn_blocking to read a line from stdin
                let tx_clone = tx.clone();
                let channel_id_clone = channel_id.clone();
                let prompt_clone = prompt.clone();

                let line_result = tokio::task::spawn_blocking(move || {
                    let mut input = String::new();
                    match io::stdin().read_line(&mut input) {
                        Ok(0) => None, // EOF
                        Ok(_) => Some(input.trim().to_string()),
                        Err(_) => None,
                    }
                });

                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        break;
                    }
                    result = line_result => {
                        match result {
                            Ok(Some(input)) => {
                                if input.is_empty() {
                                    print!("{}", prompt_clone);
                                    let _ = io::stdout().flush();
                                    continue;
                                }

                                // Check for quit commands
                                if input == "/quit" || input == "/exit" || input == "exit" || input == "quit" {
                                    println!("Goodbye!");
                                    break;
                                }

                                let sender = ChannelUser {
                                    id: "local_user".to_string(),
                                    name: Some("User".to_string()),
                                    username: None,
                                    is_self: false,
                                    trust_level: UserTrust::Owner,
                                    peer_id: None,
                                };

                                let message = ChannelMessage::text(
                                    channel_id_clone,
                                    sender,
                                    input,
                                    MessageDirection::Incoming,
                                );

                                if tx_clone.send(message).await.is_err() {
                                    break;
                                }
                            }
                            Ok(None) | Err(_) => {
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.rx = Some(rx);
        self.shutdown_tx = Some(shutdown_tx);
        self.connected = true;

        tracing::info!(channel_id = %self.id, "REPL channel connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<(), ChannelError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
        self.rx = None;
        self.connected = false;
        tracing::info!(channel_id = %self.id, "REPL channel disconnected");
        Ok(())
    }

    async fn send(&self, message: ChannelMessage) -> Result<MessageId, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        // Print the message to stdout
        println!("\n{}", message.content);
        self.print_prompt();

        Ok(message.id)
    }

    async fn receive(&mut self) -> Result<ChannelMessage, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        match &mut self.rx {
            Some(rx) => {
                rx.recv().await.ok_or(ChannelError::Closed)
            }
            None => Err(ChannelError::NotConnected),
        }
    }

    async fn try_receive(&mut self) -> Result<Option<ChannelMessage>, ChannelError> {
        if !self.connected {
            return Err(ChannelError::NotConnected);
        }

        match &mut self.rx {
            Some(rx) => {
                match rx.try_recv() {
                    Ok(msg) => Ok(Some(msg)),
                    Err(mpsc::error::TryRecvError::Empty) => Ok(None),
                    Err(mpsc::error::TryRecvError::Disconnected) => Err(ChannelError::Closed),
                }
            }
            None => Err(ChannelError::NotConnected),
        }
    }

    async fn start_typing(&self, _conversation_id: &str) -> Result<(), ChannelError> {
        // No typing indicator for REPL
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repl_channel_creation() {
        let config = ChannelConfig {
            platform: Platform::Repl,
            name: "test".to_string(),
            ..Default::default()
        };

        let channel = ReplChannel::new(config);
        assert_eq!(channel.platform(), Platform::Repl);
        assert!(!channel.is_connected());
    }
}
