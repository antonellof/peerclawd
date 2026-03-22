//! `peerclaw serve` command - Start a peer node with full distributed execution.

use clap::Args;
use futures::FutureExt;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::bootstrap;
use crate::config::Config;
use crate::db::Database;
use crate::executor::task::{ExecutionTask, ExecutionLocation, InferenceTask, TaskData};
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

    /// Load and run an agent from a TOML spec file
    #[arg(long, value_name = "PATH")]
    pub agent: Option<std::path::PathBuf>,
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    tracing::info!("Starting PeerClaw node...");

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

    // Load agent if specified
    if let Some(agent_path) = &args.agent {
        if !agent_path.exists() {
            tracing::error!("Agent spec not found: {}", agent_path.display());
        } else {
            match std::fs::read_to_string(agent_path) {
                Ok(spec_content) => {
                    let spec: toml::Value = toml::from_str(&spec_content)
                        .unwrap_or_else(|e| {
                            tracing::error!("Failed to parse agent spec: {}", e);
                            toml::Value::Table(Default::default())
                        });

                    let agent_name = spec
                        .get("agent")
                        .and_then(|a| a.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("unnamed");

                    let model = spec
                        .get("model")
                        .and_then(|m| m.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("llama-3.2-3b");

                    let agent_id = format!(
                        "agent_{}",
                        &uuid::Uuid::new_v4().to_string().replace('-', "")[..12]
                    );

                    // Store agent state in database
                    let agent_state = serde_json::json!({
                        "id": agent_id,
                        "name": agent_name,
                        "model": model,
                        "status": "running",
                        "spec_path": agent_path.display().to_string(),
                        "started_at": chrono::Utc::now().to_rfc3339(),
                    });

                    if let Ok(db) = Database::open(&config.database.path) {
                        let _ = db.store_agent(&agent_id, &agent_state);
                    }

                    tracing::info!(
                        "Agent '{}' loaded (model: {}, id: {})",
                        agent_name, model, agent_id
                    );
                }
                Err(e) => tracing::error!("Failed to read agent spec: {}", e),
            }
        }
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
    let _advertise_interval = tokio::time::interval(std::time::Duration::from_secs(30));

    // Interval for auto-accepting bids on pending jobs (after bid collection period)
    let mut bid_accept_interval = tokio::time::interval(std::time::Duration::from_secs(3));

    // Track when each job was created for bid collection timeout
    let mut job_creation_times: std::collections::HashMap<crate::job::JobId, std::time::Instant> = std::collections::HashMap::new();

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

            let local_peer_id = runtime.local_peer_id.to_string();
            let response = match runtime.execute_task(ExecutionTask::Inference(task)).await {
                Ok(result) => {
                    let provider_peer_id = match &result.location {
                        ExecutionLocation::Local => Some(local_peer_id.clone()),
                        ExecutionLocation::Remote { peer_id, .. } => Some(peer_id.clone()),
                    };
                    match &result.data {
                        TaskData::Inference(r) => InferenceResponse {
                            text: r.text.clone(),
                            tokens_generated: r.tokens_generated,
                            tokens_per_second: r.tokens_per_second as f32,
                            location: format!("{:?}", result.location),
                            provider_peer_id,
                        },
                        TaskData::Error(e) => InferenceResponse {
                            text: format!("Error: {}", e),
                            tokens_generated: 0,
                            tokens_per_second: 0.0,
                            location: "error".to_string(),
                            provider_peer_id: None,
                        },
                        _ => InferenceResponse {
                            text: "Unexpected response type".to_string(),
                            tokens_generated: 0,
                            tokens_per_second: 0.0,
                            location: "error".to_string(),
                            provider_peer_id: None,
                        },
                    }
                }
                Err(e) => InferenceResponse {
                    text: format!("Error: {}", e),
                    tokens_generated: 0,
                    tokens_per_second: 0.0,
                    location: "error".to_string(),
                    provider_peer_id: None,
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
                let pending = job_manager.pending_requests().await;
                let active = job_manager.active_jobs().await;
                let completed = job_manager.completed_jobs(100).await;

                *state.active_jobs.write().await = pending.len() + active.len();
                *state.completed_jobs.write().await = completed.len();

                // Build job list for display
                let mut job_list = Vec::new();

                // Add pending requests (awaiting bids)
                for req in &pending {
                    let requester_id = &req.requester_id;
                    let bids = job_manager.bids_count(&req.id).await;
                    job_list.push(WebJobInfo {
                        id: req.id.to_string(),
                        job_type: format!("{}", req.resource_type),
                        status: format!("Pending ({} bids)", bids),
                        provider: None,
                        requester: if requester_id.len() > 8 {
                            format!("...{}", &requester_id[requester_id.len().saturating_sub(8)..])
                        } else {
                            requester_id.clone()
                        },
                        price_micro: req.max_budget,
                        created_at: req.created_at.timestamp() as u64,
                        location: Some("Awaiting bids".to_string()),
                    });
                }

                // Add active jobs (being executed)
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
                        location: Some("Network".to_string()),
                    });
                }

                // Add completed jobs
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
                        location: Some("Network".to_string()),
                    });
                }
                *state.job_list.write().await = job_list;
            }
        }

        // Auto-accept bids for our pending requests after bid collection period
        if bid_accept_interval.tick().now_or_never().is_some() {
            auto_accept_bids(&runtime, &mut job_creation_times).await;
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
    use crate::job::network::{JobMessage, JobRequestMessage, serialize_message, topics};
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

    let job_id = request.id.clone();

    // Store request locally
    if let Err(e) = runtime.job_manager.write().await.create_request(request.clone()).await {
        tracing::error!("Job creation failed: {}", e);
        return JobSubmitResponse {
            success: false,
            job_id: None,
            error: Some(e.to_string()),
        };
    }

    // Broadcast to network via GossipSub
    let msg = JobMessage::Request(JobRequestMessage::new(request, runtime.identity.peer_id()));
    match serialize_message(&msg) {
        Ok(data) => {
            let mut network = runtime.network.write().await;
            if let Err(e) = network.publish(topics::JOB_REQUESTS, data) {
                tracing::warn!("Failed to broadcast job: {}", e);
                // Continue anyway - job is stored locally
            } else {
                tracing::info!("Job {} broadcast to network", job_id);
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize job message: {}", e);
        }
    }

    JobSubmitResponse {
        success: true,
        job_id: Some(job_id.to_string()),
        error: None,
    }
}

/// Auto-accept best bids for pending requests after bid collection period.
async fn auto_accept_bids(
    runtime: &Runtime,
    job_creation_times: &mut std::collections::HashMap<crate::job::JobId, std::time::Instant>,
) {
    use crate::job::select_best_bid;
    use crate::job::network::{JobMessage, BidAcceptedMessage, serialize_message, topics};

    // Get pending requests that might need bid acceptance
    let pending = runtime.job_manager.read().await.pending_requests().await;

    // Track creation times for new jobs
    let now = std::time::Instant::now();
    for req in &pending {
        job_creation_times.entry(req.id.clone()).or_insert(now);
    }

    // BID_COLLECTION_TIMEOUT: wait 5 seconds for bids before accepting
    const BID_COLLECTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

    for req in pending {
        // Only process requests we created (our peer ID)
        if req.requester_id != runtime.local_peer_id.to_string() {
            continue;
        }

        // Check if enough time has passed for bid collection
        let created_at = job_creation_times.get(&req.id);
        if let Some(&create_time) = created_at {
            if now.duration_since(create_time) < BID_COLLECTION_TIMEOUT {
                continue; // Still collecting bids
            }
        }

        // Get bids for this job
        let bids = runtime.job_manager.read().await.get_bids(&req.id).await;

        if bids.is_empty() {
            tracing::debug!(job_id = %req.id, "No bids received yet, waiting...");
            continue;
        }

        // Select best bid
        if let Some(best_bid) = select_best_bid(&bids, None) {
            tracing::info!(
                job_id = %req.id,
                provider = %best_bid.bidder_id,
                price = best_bid.price,
                "Auto-accepting best bid"
            );

            // Accept the bid
            match runtime.job_manager.write().await.accept_bid(&req.id, &best_bid.id).await {
                Ok(job) => {
                    // Remove from tracking
                    job_creation_times.remove(&req.id);

                    // Broadcast acceptance to network
                    let accept_msg = JobMessage::BidAccepted(BidAcceptedMessage {
                        job_id: req.id.clone(),
                        bid_id: best_bid.id.0.clone(),
                        winner_peer_id: best_bid.bidder_id.clone(),
                        escrow_id: job.escrow_id.0.clone(),
                        signature: vec![],
                    });

                    if let Ok(data) = serialize_message(&accept_msg) {
                        let mut network = runtime.network.write().await;
                        if let Err(e) = network.publish(topics::JOB_STATUS, data) {
                            tracing::warn!("Failed to broadcast bid acceptance: {}", e);
                        } else {
                            tracing::info!(job_id = %req.id, "Bid acceptance broadcast to network");
                        }
                    }

                    // If the winner is us (local execution), execute immediately
                    if best_bid.bidder_id == runtime.local_peer_id.to_string() {
                        tracing::info!(job_id = %req.id, "Executing job locally (we are the provider)");
                        let job_id = req.id.clone();
                        let payload = req.payload.clone().unwrap_or_default();
                        let resource_type = req.resource_type.clone();

                        execute_job_locally(
                            job_id,
                            resource_type,
                            payload,
                            runtime.job_manager.clone(),
                            runtime.executor.clone(),
                            runtime.network.clone(),
                            *runtime.identity.peer_id(),
                        ).await;
                    }
                }
                Err(e) => {
                    tracing::error!(job_id = %req.id, error = %e, "Failed to accept bid");
                }
            }
        }
    }
}

/// Execute a job locally when we are the provider.
async fn execute_job_locally(
    job_id: crate::job::JobId,
    resource_type: crate::job::ResourceType,
    payload: Vec<u8>,
    job_manager: std::sync::Arc<tokio::sync::RwLock<crate::job::JobManager>>,
    executor: std::sync::Arc<crate::executor::TaskExecutor>,
    network: std::sync::Arc<tokio::sync::RwLock<crate::p2p::Network>>,
    local_peer_id: libp2p::PeerId,
) {
    use crate::job::network::{JobMessage, JobResultMessage, JobStatusMessage, JobStatusUpdate, serialize_message, topics};
    use crate::job::{JobResult, ActualUsage, ExecutionMetrics};
    use crate::executor::task::{ExecutionTask, InferenceTask, TaskData, WebFetchTask};

    tracing::info!(job_id = %job_id, "Starting local job execution");

    // Mark job as in progress
    if let Err(e) = job_manager.write().await.start_job(&job_id).await {
        tracing::error!(job_id = %job_id, error = %e, "Failed to start job");
        return;
    }

    // Broadcast status update
    let status_msg = JobMessage::StatusUpdate(JobStatusMessage {
        job_id: job_id.clone(),
        status: JobStatusUpdate::Started,
        peer_id: local_peer_id.to_string(),
        timestamp: chrono::Utc::now().timestamp() as u64,
    });
    if let Ok(data) = serialize_message(&status_msg) {
        let _ = network.write().await.publish(topics::JOB_STATUS, data);
    }

    // Execute based on resource type
    let result = match resource_type {
        crate::job::ResourceType::Inference { model, tokens } => {
            // Try to parse prompt from payload
            let prompt_cow = String::from_utf8_lossy(&payload);
            let prompt: &str = if prompt_cow.is_empty() { "Hello, how are you?" } else { prompt_cow.as_ref() };

            let task = InferenceTask::new(&model, prompt)
                .with_max_tokens(tokens);

            match executor.execute(ExecutionTask::Inference(task)).await {
                Ok(task_result) => {
                    match &task_result.data {
                        TaskData::Inference(r) => {
                            JobResult::new(r.text.as_bytes().to_vec())
                                .with_usage(ActualUsage {
                                    tokens: Some(r.tokens_generated),
                                    compute_time_ms: Some(task_result.metrics.total_time_ms),
                                    bytes: None,
                                })
                                .with_metrics(ExecutionMetrics {
                                    ttfb_ms: task_result.metrics.ttfb_ms,
                                    total_time_ms: task_result.metrics.total_time_ms,
                                    tokens_per_sec: Some(r.tokens_per_second),
                                })
                        }
                        _ => JobResult::new(b"Unexpected task result type".to_vec()),
                    }
                }
                Err(e) => {
                    JobResult::new(format!("Execution error: {}", e).into_bytes())
                }
            }
        }
        crate::job::ResourceType::WebFetch { url_count: _ } => {
            // Parse URL from payload
            let url = String::from_utf8_lossy(&payload);
            let task = WebFetchTask::get(url.as_ref());

            match executor.execute(ExecutionTask::WebFetch(task)).await {
                Ok(task_result) => {
                    match &task_result.data {
                        TaskData::WebFetch(r) => {
                            JobResult::new(r.body.clone())
                                .with_usage(ActualUsage {
                                    tokens: None,
                                    compute_time_ms: Some(task_result.metrics.total_time_ms),
                                    bytes: Some(r.body.len() as u64),
                                })
                        }
                        _ => JobResult::new(b"Unexpected task result type".to_vec()),
                    }
                }
                Err(e) => {
                    JobResult::new(format!("Execution error: {}", e).into_bytes())
                }
            }
        }
        _ => {
            JobResult::new(b"Unsupported resource type for local execution".to_vec())
        }
    };

    tracing::info!(job_id = %job_id, "Job execution complete, submitting result");

    // Submit result
    if let Err(e) = job_manager.write().await.submit_result(&job_id, result.clone()).await {
        tracing::error!(job_id = %job_id, error = %e, "Failed to submit job result");
        return;
    }

    // Broadcast result to network
    let result_msg = JobMessage::Result(JobResultMessage {
        job_id: job_id.clone(),
        result: result.clone(),
        provider_peer_id: local_peer_id.to_string(),
        signature: vec![],
    });
    if let Ok(data) = serialize_message(&result_msg) {
        let _ = network.write().await.publish(topics::JOB_STATUS, data);
    }

    // Settle the job (mark as completed)
    if let Err(e) = job_manager.write().await.settle_job(&job_id, true).await {
        tracing::error!(job_id = %job_id, error = %e, "Failed to settle job");
        return;
    }

    tracing::info!(job_id = %job_id, "Job completed and settled successfully");
}
