//! Node coordinator - orchestrates all subsystems.

use std::sync::Arc;
use tokio::sync::mpsc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::ResourceMonitor;
use crate::identity::NodeIdentity;
use crate::p2p::{Network, NetworkEvent};
use crate::swarm::{SwarmManager, SwarmManagerConfig};
use crate::web;

/// The main PeerClaw node.
pub struct Node {
    config: Config,
    identity: NodeIdentity,
    database: Database,
    network: Network,
    swarm_manager: Arc<SwarmManager>,
    shutdown_tx: mpsc::Sender<()>,
    shutdown_rx: Option<mpsc::Receiver<()>>,
}

impl Node {
    /// Create a new node with the given configuration.
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        // Ensure directories exist
        bootstrap::ensure_dirs()?;

        // Load or create identity
        let identity_path = bootstrap::identity_path();
        let identity = if identity_path.exists() {
            tracing::info!("Loading identity from {:?}", identity_path);
            NodeIdentity::load(&identity_path)?
        } else {
            tracing::info!("Generating new identity");
            let identity = NodeIdentity::generate();
            identity.save(&identity_path)?;
            identity
        };

        tracing::info!("Peer ID: {}", identity.peer_id());

        // Open database
        let database = Database::open(&config.database.path)?;
        tracing::info!("Database opened at {:?}", config.database.path);

        // Create network
        let network = Network::new(&identity, config.p2p.clone())?;

        // Create swarm manager
        let swarm_config = SwarmManagerConfig {
            event_buffer_size: 256,
            max_action_history: 1000,
            track_remote_peers: true,
        };
        let swarm_manager = Arc::new(SwarmManager::new(swarm_config));
        tracing::info!("Swarm manager initialized");

        // Create shutdown channel
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        Ok(Self {
            config,
            identity,
            database,
            network,
            swarm_manager,
            shutdown_tx,
            shutdown_rx: Some(shutdown_rx),
        })
    }

    /// Get the node's peer ID.
    pub fn peer_id(&self) -> &libp2p::PeerId {
        self.identity.peer_id()
    }

    /// Get a reference to the database.
    pub fn database(&self) -> &Database {
        &self.database
    }

    /// Get a shutdown sender to signal the node to stop.
    pub fn shutdown_handle(&self) -> mpsc::Sender<()> {
        self.shutdown_tx.clone()
    }

    /// Get a reference to the swarm manager.
    pub fn swarm_manager(&self) -> &Arc<SwarmManager> {
        &self.swarm_manager
    }

    /// Run the node.
    pub async fn run(&mut self) -> anyhow::Result<()> {
        tracing::info!("Starting PeerClaw node");

        // Start network
        self.network.start().await?;

        // Subscribe to network events
        let mut event_rx = self.network.event_receiver();

        // Take shutdown receiver
        let shutdown_rx = self.shutdown_rx.take()
            .ok_or_else(|| anyhow::anyhow!("Node already running"))?;

        // Spawn network task
        let network_shutdown = self.shutdown_tx.clone();
        let mut network = std::mem::replace(
            &mut self.network,
            Network::new(&self.identity, self.config.p2p.clone())?,
        );

        let network_handle = tokio::spawn(async move {
            let (tx, rx) = mpsc::channel(1);
            if let Err(e) = network.run(rx).await {
                tracing::error!("Network error: {}", e);
            }
        });

        // Spawn web server if enabled
        let _web_handle = if self.config.web.enabled {
            let addr = self.config.web.listen_addr;
            let resource_monitor = Arc::new(ResourceMonitor::with_defaults());
            resource_monitor.start_background_updates();
            let web_state = web::create_web_state_with_swarm(
                *self.identity.peer_id(),
                resource_monitor,
                self.swarm_manager.clone(),
            );
            Some(tokio::spawn(async move {
                if let Err(e) = web::start_server(addr, web_state).await {
                    tracing::error!("Web server error: {}", e);
                }
            }))
        } else {
            None
        };

        // Register local agent in swarm
        let local_agent_id = self.swarm_manager.register_local_agent(
            format!("node-{}", self.identity.peer_id().to_string().chars().take(8).collect::<String>()),
            Default::default(),
        );
        tracing::info!("Registered local agent: {}", local_agent_id);

        // Main event loop
        tracing::info!("Node running. Press Ctrl+C to stop.");

        let swarm_manager = self.swarm_manager.clone();
        tokio::select! {
            // Handle network events and bridge to swarm
            _ = async {
                while let Ok(event) = event_rx.recv().await {
                    tracing::debug!("Network event: {}", event);

                    // Bridge P2P events to swarm manager
                    swarm_manager.handle_network_event(event);
                }
            } => {}

            // Handle shutdown signal
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received Ctrl+C, shutting down...");
            }
        }

        // Cleanup
        tracing::info!("Node stopped");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_node_creation() {
        let dir = tempdir().unwrap();
        let mut config = Config::default();
        config.database.path = dir.path().join("test.redb");

        let node = Node::new(config).await;
        assert!(node.is_ok());
    }
}
