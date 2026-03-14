//! `peerclawd serve` command - Start a peer node with full distributed execution.

use clap::Args;
use futures::FutureExt;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::{ExecutionTask, InferenceTask, TaskData};
use crate::identity::NodeIdentity;
use crate::job::PricingStrategy;
use crate::p2p::NetworkEvent;
use crate::runtime::Runtime;
use crate::web::{InferenceRequest, InferenceResponse, JobSubmitRequest, JobSubmitResponse, WebJobInfo};

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

    // Event receiver is available for external listeners
    let _event_rx = runtime.network.read().await.event_receiver();

    // Create channels for web UI
    let (inference_tx, mut inference_rx) = mpsc::channel::<InferenceRequest>(32);
    let (job_submit_tx, mut job_submit_rx) = mpsc::channel::<JobSubmitRequest>(32);

    // Create web state for dashboard with full channel support
    let web_state = if config.web.enabled {
        Some(crate::web::create_web_state_with_channels(
            runtime.local_peer_id,
            runtime.executor.resource_monitor(),
            inference_tx,
            job_submit_tx,
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

    // Interval for advertising resources
    let mut advertise_interval = tokio::time::interval(std::time::Duration::from_secs(30));

    // Run event loop until shutdown - poll swarm directly
    loop {
        // Get write lock for swarm polling
        let swarm_event = {
            let mut network = runtime.network.write().await;
            // Use poll_next to check for events without blocking forever
            use futures::StreamExt;
            tokio::select! {
                biased;

                event = network.swarm.select_next_some() => Some(event),
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => None,
            }
        };

        // Process swarm event if we got one
        if let Some(event) = swarm_event {
            let network_event = {
                let mut network = runtime.network.write().await;
                network.process_swarm_event(event).await
            };

            if let Some(e) = network_event {
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
        }

        // Handle inference requests from web UI (process inline - one at a time)
        if let Ok(request) = inference_rx.try_recv() {
            let task = InferenceTask::new(&request.model, &request.prompt)
                .with_max_tokens(request.max_tokens)
                .with_temperature(request.temperature);

            let response = match runtime.execute_task(ExecutionTask::Inference(task)).await {
                Ok(result) => {
                    match &result.data {
                        TaskData::Inference(r) => InferenceResponse {
                            text: r.text.clone(),
                            tokens_generated: r.tokens_generated,
                            tokens_per_second: r.tokens_per_second as f32,
                            location: format!("{:?}", result.location),
                        },
                        TaskData::Error(e) => InferenceResponse {
                            text: format!("Error: {}", e),
                            tokens_generated: 0,
                            tokens_per_second: 0.0,
                            location: "error".to_string(),
                        },
                        _ => InferenceResponse {
                            text: "Unexpected response type".to_string(),
                            tokens_generated: 0,
                            tokens_per_second: 0.0,
                            location: "error".to_string(),
                        },
                    }
                }
                Err(e) => InferenceResponse {
                    text: format!("Error: {}", e),
                    tokens_generated: 0,
                    tokens_per_second: 0.0,
                    location: "error".to_string(),
                },
            };

            let _ = request.response_tx.send(response);
        }

        // Handle job submission requests from web UI
        if let Ok(request) = job_submit_rx.try_recv() {
            let response = handle_job_submit(&runtime, request.job_type, request.budget, request.payload).await;
            let _ = request.response_tx.send(response);
        }

        // Check for shutdown signal
        if tokio::signal::ctrl_c().now_or_never().is_some() {
            tracing::info!("Received Ctrl+C, shutting down...");
            break;
        }

        // Periodically update web state
        if stats_interval.tick().now_or_never().is_some() {
            if let Some(ref state) = web_state {
                *state.wallet_balance.write().await = runtime.balance().await;

                // Update job counts and list
                let job_manager = runtime.job_manager.read().await;
                let active = job_manager.active_jobs().await;
                let completed = job_manager.completed_jobs(100).await;

                *state.active_jobs.write().await = active.len();
                *state.completed_jobs.write().await = completed.len();

                // Build job list for display
                let mut job_list = Vec::new();
                for job in &active {
                    let provider_id = &job.bid.bidder_id;
                    let requester_id = &job.request.requester_id;
                    job_list.push(WebJobInfo {
                        id: job.id.to_string(),
                        job_type: format!("{}", job.request.resource_type),
                        status: format!("{}", job.status),
                        provider: Some(format!("...{}", &provider_id[provider_id.len().saturating_sub(8)..])),
                        requester: format!("...{}", &requester_id[requester_id.len().saturating_sub(8)..]),
                        price_micro: job.bid.price,
                        created_at: job.created_at.timestamp() as u64,
                        location: None,
                    });
                }
                for job in &completed {
                    let provider_id = &job.bid.bidder_id;
                    let requester_id = &job.request.requester_id;
                    job_list.push(WebJobInfo {
                        id: job.id.to_string(),
                        job_type: format!("{}", job.request.resource_type),
                        status: format!("{}", job.status),
                        provider: Some(format!("...{}", &provider_id[provider_id.len().saturating_sub(8)..])),
                        requester: format!("...{}", &requester_id[requester_id.len().saturating_sub(8)..]),
                        price_micro: job.bid.price,
                        created_at: job.created_at.timestamp() as u64,
                        location: None,
                    });
                }
                *state.job_list.write().await = job_list;
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

/// Handle a job submission from the web UI.
async fn handle_job_submit(
    runtime: &Runtime,
    job_type: String,
    budget: f64,
    payload: String,
) -> JobSubmitResponse {
    use crate::job::{ResourceType, JobRequest};
    use crate::wallet::to_micro;

    tracing::info!("Web UI job submission: type={}, budget={}", job_type, budget);

    // Create resource type based on job type
    let resource_type = match job_type.as_str() {
        "inference" => ResourceType::Inference {
            model: "llama-3.2-3b".to_string(),
            tokens: 500,
        },
        "web_fetch" => ResourceType::WebFetch {
            url_count: 1,
        },
        "wasm" => ResourceType::WasmTool {
            tool_name: payload.clone(),
            invocations: 1,
        },
        _ => {
            return JobSubmitResponse {
                success: false,
                job_id: None,
                error: Some(format!("Unknown job type: {}", job_type)),
            };
        }
    };

    // Create job request
    let request = JobRequest::new(
        resource_type,
        to_micro(budget),
        300, // 5 minute timeout
    )
    .with_requester(runtime.local_peer_id.to_string())
    .with_payload(payload.into_bytes());

    // Submit to job manager
    match runtime.job_manager.write().await.create_request(request).await {
        Ok(job_id) => {
            tracing::info!("Job submitted: {}", job_id);
            JobSubmitResponse {
                success: true,
                job_id: Some(job_id.to_string()),
                error: None,
            }
        }
        Err(e) => {
            tracing::error!("Job submission failed: {}", e);
            JobSubmitResponse {
                success: false,
                job_id: None,
                error: Some(e.to_string()),
            }
        }
    }
}
