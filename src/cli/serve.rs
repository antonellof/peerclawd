//! `peerclawd serve` command - Start a peer node with full distributed execution.

use clap::Args;
use std::net::SocketAddr;
use std::sync::Arc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::identity::NodeIdentity;
use crate::job::PricingStrategy;
use crate::p2p::NetworkEvent;
use crate::runtime::Runtime;

#[derive(Args)]
pub struct ServeArgs {
    /// Advertise GPU resources
    #[arg(long)]
    pub gpu: bool,

    /// Limit CPU contribution (number of cores)
    #[arg(long)]
    pub cpu: Option<u16>,

    /// Allocate distributed storage (e.g., "50GB")
    #[arg(long)]
    pub storage: Option<String>,

    /// Enable embedded web UI on this address
    #[arg(long, value_name = "ADDR")]
    pub web: Option<SocketAddr>,

    /// Join existing network via known peer
    #[arg(long, value_name = "MULTIADDR")]
    pub bootstrap: Option<String>,

    /// Path to wallet keyfile
    #[arg(long, value_name = "PATH")]
    pub wallet: Option<std::path::PathBuf>,

    /// Listen address for P2P (default: /ip4/0.0.0.0/tcp/0)
    #[arg(long, value_name = "MULTIADDR")]
    pub listen: Option<String>,

    /// Accept jobs from network (act as provider)
    #[arg(long)]
    pub provider: bool,

    /// Base price per token in μPCLAW (default: 100)
    #[arg(long, default_value = "100")]
    pub price_per_token: u64,
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    tracing::info!("Starting PeerClaw'd node...");

    // Ensure directories exist
    bootstrap::ensure_dirs()?;

    // Load or create identity
    let identity_path = bootstrap::identity_path();
    let identity = if identity_path.exists() {
        tracing::info!("Loading identity from {:?}", identity_path);
        Arc::new(NodeIdentity::load(&identity_path)?)
    } else {
        tracing::info!("Generating new identity");
        let id = NodeIdentity::generate();
        id.save(&identity_path)?;
        Arc::new(id)
    };

    tracing::info!("Peer ID: {}", identity.peer_id());

    // Load configuration
    let mut config = Config::load()?;

    // Apply CLI overrides
    if let Some(web_addr) = args.web {
        config.web.enabled = true;
        config.web.listen_addr = web_addr;
    }

    if let Some(bootstrap) = args.bootstrap {
        config.p2p.bootstrap_peers.push(bootstrap);
    }

    if let Some(listen) = args.listen {
        // Replace default listen addresses with specified one
        config.p2p.listen_addresses = vec![listen];
    }

    if args.gpu {
        config.resources.advertise_gpu = true;
    }

    if let Some(cpu) = args.cpu {
        config.resources.cpu_cores = Some(cpu);
    }

    // Open database
    let database = Database::open(&config.database.path)?;
    tracing::info!("Database opened at {:?}", config.database.path);

    // Create runtime with all subsystems
    let runtime = Runtime::new(identity, database, config.clone()).await?;

    // Set pricing strategy if acting as provider
    if args.provider {
        let mut pricing = PricingStrategy::default();
        pricing.base_rates.inference_small_per_1k = args.price_per_token;
        runtime.set_pricing(pricing).await;
        tracing::info!("Acting as job provider (price: {} μPCLAW/1k tokens)", args.price_per_token);
    }

    // Subscribe to job marketplace topics
    runtime.subscribe_to_job_topics().await?;

    // Start the network
    {
        let mut network = runtime.network.write().await;
        network.start().await?;
    }

    // Get event receiver
    let mut event_rx = runtime.network.read().await.event_receiver();

    // Create web state for dashboard
    let web_state = if config.web.enabled {
        Some(crate::web::create_web_state(
            runtime.local_peer_id,
            runtime.executor.resource_monitor(),
        ))
    } else {
        None
    };

    // Start web server if enabled
    if let Some(state) = web_state.clone() {
        let web_addr = config.web.listen_addr;
        tokio::spawn(async move {
            if let Err(e) = crate::web::start_server(web_addr, state).await {
                tracing::error!("Web server error: {}", e);
            }
        });
        tracing::info!("Web UI available at http://{}", config.web.listen_addr);
    }

    // Main loop
    tracing::info!("Node running. Press Ctrl+C to stop.");

    let stats = runtime.stats().await;
    tracing::info!(
        "Balance: {:.6} PCLAW | Connected peers: {}",
        stats.balance,
        stats.connected_peers
    );

    // Interval for updating web state
    let mut stats_interval = tokio::time::interval(std::time::Duration::from_secs(2));

    // Run event loop until shutdown
    loop {
        tokio::select! {
            // Handle network events
            event = event_rx.recv() => {
                match event {
                    Ok(e) => {
                        // Update web state peer list if available
                        if let Some(ref state) = web_state {
                            if let NetworkEvent::PeerConnected(peer_id) = &e {
                                state.connected_peers.write().await.push(*peer_id);
                            } else if let NetworkEvent::PeerDisconnected(peer_id) = &e {
                                state.connected_peers.write().await.retain(|p| p != peer_id);
                            }
                        }
                        handle_network_event(&runtime, e).await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Lagged {} events", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        tracing::info!("Event channel closed");
                        break;
                    }
                }
            }
            // Periodically update web state
            _ = stats_interval.tick() => {
                if let Some(ref state) = web_state {
                    *state.wallet_balance.write().await = runtime.balance().await;
                    *state.active_jobs.write().await = runtime.job_manager.read().await.active_jobs().await.len();
                    *state.completed_jobs.write().await = runtime.job_manager.read().await.completed_jobs(100).await.len();
                }
            }
            // Handle shutdown signal
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Received Ctrl+C, shutting down...");
                break;
            }
        }
    }

    let final_stats = runtime.stats().await;
    tracing::info!(
        "Final stats - Balance: {:.6} PCLAW | Active jobs: {} | Completed jobs: {}",
        final_stats.balance,
        final_stats.active_jobs,
        final_stats.completed_jobs
    );

    tracing::info!("Node stopped");
    Ok(())
}

/// Handle a network event.
async fn handle_network_event(runtime: &Runtime, event: NetworkEvent) {
    match event {
        NetworkEvent::PeerConnected(peer_id) => {
            tracing::info!("Peer connected: {}", peer_id);
        }
        NetworkEvent::PeerDisconnected(peer_id) => {
            tracing::info!("Peer disconnected: {}", peer_id);
        }
        NetworkEvent::PeerDiscovered { peer_id, addresses } => {
            tracing::debug!(
                "Discovered peer {} at {} addresses",
                peer_id,
                addresses.len()
            );
        }
        NetworkEvent::GossipMessage { topic, data, source } => {
            tracing::debug!(
                "Gossip message on topic '{}' ({} bytes) from {:?}",
                topic,
                data.len(),
                source
            );

            // Handle job-related messages
            runtime.handle_gossip_message(&topic, data, source).await;
        }
        NetworkEvent::RequestReceived { request_id, from, payload } => {
            tracing::debug!(
                "Request {} received from {} ({} bytes)",
                request_id,
                from,
                payload.len()
            );
        }
        NetworkEvent::ResourceAdvertised { peer_id, manifest } => {
            tracing::debug!(
                "Resources advertised by {}: {:?}",
                peer_id,
                manifest
            );
        }
    }
}
