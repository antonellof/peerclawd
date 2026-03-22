//! Web dashboard with real-time monitoring and chat interface.
//!
//! Provides a web UI for:
//! - Network topology visualization
//! - CPU/GPU/RAM monitoring
//! - Job marketplace status
//! - AI chat interface
//! - OpenAI-compatible API (/v1/chat/completions, /v1/models)

pub mod openai;

use std::sync::Arc;
use std::net::SocketAddr;


use axum::{
    extract::{State, ws::{WebSocket, WebSocketUpgrade, Message}},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, mpsc};
use libp2p::PeerId;

use crate::executor::ResourceMonitor;
use crate::swarm::SwarmManager;
use crate::wallet::from_micro;

/// Request for inference from web UI
pub struct InferenceRequest {
    pub prompt: String,
    pub model: String,
    pub max_tokens: u32,
    pub temperature: f32,
    pub response_tx: tokio::sync::oneshot::Sender<InferenceResponse>,
}

/// Response to inference request
pub struct InferenceResponse {
    pub text: String,
    pub tokens_generated: u32,
    pub tokens_per_second: f32,
    pub location: String,
    pub provider_peer_id: Option<String>,
}

/// Request for job submission from web UI
pub struct JobSubmitRequest {
    pub job_type: String,
    pub budget: f64,
    pub payload: String,
    pub response_tx: tokio::sync::oneshot::Sender<JobSubmitResponse>,
}

/// Response to job submission
pub struct JobSubmitResponse {
    pub success: bool,
    pub job_id: Option<String>,
    pub error: Option<String>,
}

/// Detailed job information for display.
#[derive(Clone, Serialize)]
pub struct WebJobInfo {
    pub id: String,
    pub job_type: String,
    pub status: String,
    pub provider: Option<String>,
    pub requester: String,
    pub price_micro: u64,
    pub created_at: u64,
    pub location: Option<String>,
}

/// Web server state - contains only Send/Sync safe components.
pub struct WebState {
    pub local_peer_id: PeerId,
    pub resource_monitor: Arc<ResourceMonitor>,
    pub wallet_balance: Arc<RwLock<u64>>,
    /// Connected peers (updated periodically)
    pub connected_peers: Arc<RwLock<Vec<PeerId>>>,
    /// Active jobs count
    pub active_jobs: Arc<RwLock<usize>>,
    /// Completed jobs count
    pub completed_jobs: Arc<RwLock<usize>>,
    /// Detailed job list for display
    pub job_list: Arc<RwLock<Vec<WebJobInfo>>>,
    /// Channel for receiving inference requests from web UI
    pub inference_tx: Option<mpsc::Sender<InferenceRequest>>,
    /// Channel for receiving job submission requests from web UI
    pub job_submit_tx: Option<mpsc::Sender<JobSubmitRequest>>,
    /// Swarm manager for agent visualization
    pub swarm_manager: Option<Arc<SwarmManager>>,
}

/// Create the web router.
pub fn create_router(state: Arc<WebState>) -> Router {
    Router::new()
        // Dashboard routes
        .route("/", get(index))
        .route("/chat", get(chat_page))
        .route("/api/status", get(api_status))
        .route("/api/peers", get(api_peers))
        .route("/api/jobs", get(api_jobs))
        .route("/api/jobs/submit", post(api_submit_job))
        .route("/api/chat", post(api_chat))
        .route("/ws", get(ws_handler))
        // Swarm visualization routes
        .route("/api/swarm/agents", get(api_swarm_agents))
        .route("/api/swarm/topology", get(api_swarm_topology))
        .route("/api/swarm/timeline", get(api_swarm_timeline))
        // OpenAI-compatible API routes
        .route("/v1/chat/completions", post(openai::chat_completions))
        .route("/v1/models", get(openai::list_models))
        .route("/v1/embeddings", post(openai::embeddings))
        .with_state(state)
}

/// Create WebState for the dashboard (basic version without inference).
pub fn create_web_state(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: None,
        job_submit_tx: None,
        swarm_manager: None,
    })
}

/// Create WebState with inference and job submission channels.
pub fn create_web_state_with_channels(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
    inference_tx: mpsc::Sender<InferenceRequest>,
    job_submit_tx: mpsc::Sender<JobSubmitRequest>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: Some(inference_tx),
        job_submit_tx: Some(job_submit_tx),
        swarm_manager: None,
    })
}

/// Create WebState with inference channel only (legacy).
pub fn create_web_state_with_inference(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
    inference_tx: mpsc::Sender<InferenceRequest>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: Some(inference_tx),
        job_submit_tx: None,
        swarm_manager: None,
    })
}

/// Create WebState with swarm manager for agent visualization.
pub fn create_web_state_with_swarm(
    local_peer_id: PeerId,
    resource_monitor: Arc<ResourceMonitor>,
    swarm_manager: Arc<SwarmManager>,
) -> Arc<WebState> {
    Arc::new(WebState {
        local_peer_id,
        resource_monitor,
        wallet_balance: Arc::new(RwLock::new(0)),
        connected_peers: Arc::new(RwLock::new(Vec::new())),
        active_jobs: Arc::new(RwLock::new(0)),
        completed_jobs: Arc::new(RwLock::new(0)),
        job_list: Arc::new(RwLock::new(Vec::new())),
        inference_tx: None,
        job_submit_tx: None,
        swarm_manager: Some(swarm_manager),
    })
}

/// Start the web server.
pub async fn start_server(
    addr: SocketAddr,
    state: Arc<WebState>,
) -> anyhow::Result<()> {
    let app = create_router(state);

    tracing::info!("Web UI starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// === API Endpoints ===

async fn index() -> Html<&'static str> {
    Html(include_str!("dashboard.html"))
}

async fn chat_page() -> Html<&'static str> {
    Html(include_str!("chat.html"))
}

#[derive(Serialize)]
struct StatusResponse {
    peer_id: String,
    connected_peers: usize,
    balance: f64,
    cpu_usage: f64,
    ram_used_mb: u32,
    ram_total_mb: u32,
    gpu_usage: Option<f64>,
    active_jobs: usize,
    completed_jobs: usize,
    active_inference: u32,
    active_web: u32,
    active_wasm: u32,
}

async fn api_status(State(state): State<Arc<WebState>>) -> Json<StatusResponse> {
    let resource_state = state.resource_monitor.current_state().await;
    let balance = from_micro(*state.wallet_balance.read().await);
    let connected_peers = state.connected_peers.read().await.len();
    let active_jobs = *state.active_jobs.read().await;
    let completed_jobs = *state.completed_jobs.read().await;

    Json(StatusResponse {
        peer_id: state.local_peer_id.to_string(),
        connected_peers,
        balance,
        cpu_usage: resource_state.cpu_usage,
        ram_used_mb: resource_state.ram_total_mb - resource_state.ram_available_mb,
        ram_total_mb: resource_state.ram_total_mb,
        gpu_usage: resource_state.gpu_usage,
        active_jobs,
        completed_jobs,
        active_inference: resource_state.active_inference_tasks,
        active_web: resource_state.active_web_tasks,
        active_wasm: resource_state.active_wasm_tasks,
    })
}

#[derive(Serialize)]
struct PeerInfo {
    id: String,
    connected: bool,
}

async fn api_peers(State(state): State<Arc<WebState>>) -> Json<Vec<PeerInfo>> {
    let peers = state.connected_peers.read().await;
    let peer_infos: Vec<PeerInfo> = peers
        .iter()
        .map(|p| PeerInfo {
            id: p.to_string(),
            connected: true,
        })
        .collect();
    Json(peer_infos)
}

async fn api_jobs(State(state): State<Arc<WebState>>) -> Json<Vec<WebJobInfo>> {
    let jobs = state.job_list.read().await;
    Json(jobs.clone())
}

#[derive(Deserialize)]
struct JobSubmitPayload {
    job_type: String,
    budget: f64,
    payload: String,
}

#[derive(Serialize)]
struct JobSubmitResult {
    success: bool,
    job_id: Option<String>,
    error: Option<String>,
}

async fn api_submit_job(
    State(state): State<Arc<WebState>>,
    Json(req): Json<JobSubmitPayload>,
) -> Json<JobSubmitResult> {
    // If we have a job submission channel, use it
    if let Some(tx) = &state.job_submit_tx {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = JobSubmitRequest {
            job_type: req.job_type,
            budget: req.budget,
            payload: req.payload,
            response_tx,
        };

        if tx.send(request).await.is_ok() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(30),
                response_rx,
            ).await {
                Ok(Ok(response)) => {
                    return Json(JobSubmitResult {
                        success: response.success,
                        job_id: response.job_id,
                        error: response.error,
                    });
                }
                Ok(Err(_)) => {
                    return Json(JobSubmitResult {
                        success: false,
                        job_id: None,
                        error: Some("Job submission cancelled".to_string()),
                    });
                }
                Err(_) => {
                    return Json(JobSubmitResult {
                        success: false,
                        job_id: None,
                        error: Some("Job submission timeout".to_string()),
                    });
                }
            }
        }
    }

    // Fallback: no job submission channel
    Json(JobSubmitResult {
        success: false,
        job_id: None,
        error: Some("Job submission not available. Restart node with full features.".to_string()),
    })
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    model: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    tokens: u32,
    tokens_per_second: f32,
    location: String,
    provider_peer_id: Option<String>,
}

async fn api_chat(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ChatRequest>,
) -> Json<ChatResponse> {
    let model = req.model.unwrap_or_else(|| "llama-3.2-3b".to_string());
    let max_tokens = req.max_tokens.unwrap_or(500);
    let temperature = req.temperature.unwrap_or(0.7);

    // If we have an inference channel, use it
    if let Some(tx) = &state.inference_tx {
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        let request = InferenceRequest {
            prompt: req.message,
            model: model.clone(),
            max_tokens,
            temperature,
            response_tx,
        };

        if tx.send(request).await.is_ok() {
            match tokio::time::timeout(
                std::time::Duration::from_secs(60),
                response_rx,
            ).await {
                Ok(Ok(response)) => {
                    return Json(ChatResponse {
                        response: response.text,
                        tokens: response.tokens_generated,
                        tokens_per_second: response.tokens_per_second,
                        location: response.location,
                        provider_peer_id: response.provider_peer_id,
                    });
                }
                Ok(Err(_)) => {
                    return Json(ChatResponse {
                        response: "Error: Inference task cancelled".to_string(),
                        tokens: 0,
                        tokens_per_second: 0.0,
                        location: "error".to_string(),
                        provider_peer_id: None,
                    });
                }
                Err(_) => {
                    return Json(ChatResponse {
                        response: "Error: Inference timeout (60s)".to_string(),
                        tokens: 0,
                        tokens_per_second: 0.0,
                        location: "error".to_string(),
                        provider_peer_id: None,
                    });
                }
            }
        }
    }

    // Fallback: direct users to CLI
    Json(ChatResponse {
        response: format!(
            "Chat is available via CLI: peerclaw chat --model {}\n\n\
            To enable web chat, restart the node with inference support.",
            model
        ),
        tokens: 0,
        tokens_per_second: 0.0,
        location: "none".to_string(),
        provider_peer_id: None,
    })
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WebState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<WebState>) {
    // Send status updates every second
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));

    loop {
        interval.tick().await;

        let resource_state = state.resource_monitor.current_state().await;
        let connected_peers = state.connected_peers.read().await.len();
        let active_jobs = *state.active_jobs.read().await;

        let status = serde_json::json!({
            "type": "status",
            "data": {
                "cpu_usage": resource_state.cpu_usage,
                "ram_used_mb": resource_state.ram_total_mb - resource_state.ram_available_mb,
                "ram_total_mb": resource_state.ram_total_mb,
                "connected_peers": connected_peers,
                "active_jobs": active_jobs,
            }
        });

        if socket.send(Message::Text(status.to_string())).await.is_err() {
            break;
        }
    }
}

// === Swarm API Endpoints ===

#[derive(Serialize)]
struct SwarmAgentInfo {
    id: String,
    name: String,
    state: String,
    is_local: bool,
    action_count: u64,
    jobs_completed: u64,
    jobs_failed: u64,
    success_rate: f64,
    created_at: String,
    last_active_at: String,
}

#[derive(Serialize)]
struct SwarmAgentsResponse {
    agents: Vec<SwarmAgentInfo>,
    total: usize,
}

async fn api_swarm_agents(State(state): State<Arc<WebState>>) -> Json<SwarmAgentsResponse> {
    let Some(swarm) = &state.swarm_manager else {
        return Json(SwarmAgentsResponse { agents: vec![], total: 0 });
    };

    let agents = swarm.get_agents();
    let agent_infos: Vec<SwarmAgentInfo> = agents
        .into_iter()
        .map(|a| SwarmAgentInfo {
            id: a.id.to_string(),
            name: a.name.clone(),
            state: a.state_display().to_string(),
            is_local: a.peer_id.is_none(),
            action_count: a.action_count,
            jobs_completed: a.jobs_completed,
            jobs_failed: a.jobs_failed,
            success_rate: a.success_rate(),
            created_at: a.created_at.to_rfc3339(),
            last_active_at: a.last_active_at.to_rfc3339(),
        })
        .collect();

    let total = agent_infos.len();
    Json(SwarmAgentsResponse { agents: agent_infos, total })
}

#[derive(Serialize)]
struct TopologyNode {
    id: String,
    name: String,
    state: String,
    is_local: bool,
    action_count: u64,
    success_rate: f64,
}

#[derive(Serialize)]
struct TopologyEdge {
    source: String,
    target: String,
}

#[derive(Serialize)]
struct SwarmTopologyResponse {
    nodes: Vec<TopologyNode>,
    edges: Vec<TopologyEdge>,
    timestamp: String,
}

async fn api_swarm_topology(State(state): State<Arc<WebState>>) -> Json<SwarmTopologyResponse> {
    let Some(swarm) = &state.swarm_manager else {
        return Json(SwarmTopologyResponse {
            nodes: vec![],
            edges: vec![],
            timestamp: chrono::Utc::now().to_rfc3339(),
        });
    };

    let agents = swarm.get_agents();
    let nodes: Vec<TopologyNode> = agents
        .iter()
        .map(|a| TopologyNode {
            id: a.id.to_string(),
            name: a.name.clone(),
            state: a.state_display().to_string(),
            is_local: a.peer_id.is_none(),
            action_count: a.action_count,
            success_rate: a.success_rate(),
        })
        .collect();

    // Build edges: connect local agents to remote agents
    let mut edges = Vec::new();
    let local_agents: Vec<_> = agents.iter().filter(|a| a.peer_id.is_none()).collect();
    let remote_agents: Vec<_> = agents.iter().filter(|a| a.peer_id.is_some()).collect();

    for local in &local_agents {
        for remote in &remote_agents {
            edges.push(TopologyEdge {
                source: local.id.to_string(),
                target: remote.id.to_string(),
            });
        }
    }

    Json(SwarmTopologyResponse {
        nodes,
        edges,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

#[derive(Serialize)]
struct SwarmActionInfo {
    id: String,
    agent_id: String,
    agent_name: String,
    action_type: String,
    details: String,
    timestamp: String,
}

#[derive(Serialize)]
struct SwarmTimelineResponse {
    actions: Vec<SwarmActionInfo>,
    total: usize,
    has_more: bool,
}

async fn api_swarm_timeline(State(state): State<Arc<WebState>>) -> Json<SwarmTimelineResponse> {
    let Some(swarm) = &state.swarm_manager else {
        return Json(SwarmTimelineResponse {
            actions: vec![],
            total: 0,
            has_more: false,
        });
    };

    let actions = swarm.get_actions(50, 0);
    let action_infos: Vec<SwarmActionInfo> = actions
        .into_iter()
        .map(|a| SwarmActionInfo {
            id: a.id.to_string(),
            agent_id: a.agent_id.to_string(),
            agent_name: a.agent_name,
            action_type: format!("{:?}", a.action_type),
            details: a.description,
            timestamp: a.timestamp.to_rfc3339(),
        })
        .collect();

    let total = action_infos.len();
    Json(SwarmTimelineResponse {
        actions: action_infos,
        total,
        has_more: false,
    })
}
