//! Web dashboard with real-time monitoring and chat interface.
//!
//! Provides a web UI for:
//! - Network topology visualization
//! - CPU/GPU/RAM monitoring
//! - Job marketplace status
//! - AI chat interface

use std::sync::Arc;
use std::net::SocketAddr;

use axum::{
    extract::{State, ws::{WebSocket, WebSocketUpgrade, Message}},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use libp2p::PeerId;

use crate::executor::{ResourceMonitor, ResourceState};
use crate::wallet::from_micro;

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
}

/// Create the web router.
pub fn create_router(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(api_status))
        .route("/api/peers", get(api_peers))
        .route("/api/jobs", get(api_jobs))
        .route("/api/chat", post(api_chat))
        .route("/ws", get(ws_handler))
        .with_state(state)
}

/// Create WebState for the dashboard.
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

#[derive(Serialize)]
struct JobInfo {
    id: String,
    status: String,
    provider: String,
    price: f64,
}

async fn api_jobs(State(_state): State<Arc<WebState>>) -> Json<Vec<JobInfo>> {
    // Jobs list is not available in simplified web state
    // Return empty for now - could be enhanced with a channel-based update mechanism
    Json(vec![])
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    model: Option<String>,
}

#[derive(Serialize)]
struct ChatResponse {
    response: String,
    tokens: u32,
    time_ms: u64,
}

async fn api_chat(
    State(_state): State<Arc<WebState>>,
    Json(req): Json<ChatRequest>,
) -> Json<ChatResponse> {
    // Chat functionality requires full runtime access
    // For now, return a placeholder message directing users to CLI
    let model = req.model.unwrap_or_else(|| "llama-3.2-3b".to_string());
    Json(ChatResponse {
        response: format!(
            "Chat is available via CLI: peerclawd chat --model {}\n\n\
            Web-based chat will be available in a future update.",
            model
        ),
        tokens: 0,
        time_ms: 0,
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
