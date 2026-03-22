//! HTTP 402 Payment Required proxy.
//!
//! Implements a pay-per-request proxy that requires PCLAW token
//! payment for API access. Supports both per-request payment
//! and payment channel-based streaming access.

mod handler;
mod payment;
mod pricing;

pub use handler::ProxyHandler;
pub use payment::{PaymentMethod, PaymentProof};
pub use pricing::{EndpointPricing, ProxyPricing};

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

use crate::channel::ChannelManager;
use crate::wallet::{from_micro, to_micro, Wallet};

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("Payment required: {0}")]
    PaymentRequired(String),

    #[error("Invalid payment proof: {0}")]
    InvalidPayment(String),

    #[error("Insufficient payment: required {required}, provided {provided}")]
    InsufficientPayment { required: u64, provided: u64 },

    #[error("Endpoint not found: {0}")]
    EndpointNotFound(String),

    #[error("Upstream error: {0}")]
    UpstreamError(String),

    #[error("Rate limit exceeded")]
    RateLimited,

    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ProxyError::PaymentRequired(msg) => (StatusCode::PAYMENT_REQUIRED, msg.clone()),
            ProxyError::InvalidPayment(msg) => (StatusCode::UNAUTHORIZED, msg.clone()),
            ProxyError::InsufficientPayment { required, provided } => (
                StatusCode::PAYMENT_REQUIRED,
                format!(
                    "Insufficient payment: need {:.6} PCLAW, got {:.6} PCLAW",
                    from_micro(*required),
                    from_micro(*provided)
                ),
            ),
            ProxyError::EndpointNotFound(path) => {
                (StatusCode::NOT_FOUND, format!("Endpoint not found: {}", path))
            }
            ProxyError::UpstreamError(msg) => (StatusCode::BAD_GATEWAY, msg.clone()),
            ProxyError::RateLimited => (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded".into()),
            ProxyError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone()),
        };

        (status, message).into_response()
    }
}

/// Configuration for the HTTP 402 proxy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Listen address for the proxy server
    pub listen_addr: SocketAddr,
    /// Base price per request in μPCLAW (default pricing)
    pub base_price_per_request: u64,
    /// Enable payment channels for streaming access
    pub enable_channels: bool,
    /// Minimum channel capacity for streaming
    pub min_channel_capacity: u64,
    /// Rate limit: max requests per minute per client
    pub rate_limit_per_minute: u32,
    /// Allow free tier (limited requests without payment)
    pub free_tier_enabled: bool,
    /// Free tier requests per hour
    pub free_tier_requests_per_hour: u32,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:8402".parse().unwrap(),
            base_price_per_request: to_micro(0.01), // 0.01 PCLAW per request
            enable_channels: true,
            min_channel_capacity: to_micro(10.0), // 10 PCLAW minimum channel
            rate_limit_per_minute: 100,
            free_tier_enabled: true,
            free_tier_requests_per_hour: 10,
        }
    }
}

/// Shared state for the proxy server.
pub struct ProxyState {
    /// Wallet for receiving payments
    pub wallet: Arc<Wallet>,
    /// Channel manager for streaming payments
    pub channels: Option<Arc<ChannelManager>>,
    /// Pricing configuration
    pub pricing: ProxyPricing,
    /// Proxy configuration
    pub config: ProxyConfig,
    /// Free tier usage tracking: client_id -> (hour_timestamp, count)
    pub free_tier_usage: RwLock<HashMap<String, (u64, u32)>>,
    /// Rate limit tracking: client_id -> (minute_timestamp, count)
    pub rate_limits: RwLock<HashMap<String, (u64, u32)>>,
}

impl ProxyState {
    /// Create new proxy state.
    pub fn new(
        wallet: Arc<Wallet>,
        channels: Option<Arc<ChannelManager>>,
        config: ProxyConfig,
    ) -> Self {
        Self {
            wallet,
            channels,
            pricing: ProxyPricing::default(),
            config,
            free_tier_usage: RwLock::new(HashMap::new()),
            rate_limits: RwLock::new(HashMap::new()),
        }
    }

    /// Check if client has free tier allowance remaining.
    pub async fn check_free_tier(&self, client_id: &str) -> bool {
        if !self.config.free_tier_enabled {
            return false;
        }

        let now = chrono::Utc::now().timestamp() as u64;
        let current_hour = now / 3600;

        let usage = self.free_tier_usage.write().await;

        match usage.get(client_id) {
            Some((hour, count)) if *hour == current_hour => {
                *count < self.config.free_tier_requests_per_hour
            }
            _ => true, // New hour or new client
        }
    }

    /// Consume a free tier request.
    pub async fn use_free_tier(&self, client_id: &str) {
        let now = chrono::Utc::now().timestamp() as u64;
        let current_hour = now / 3600;

        let mut usage = self.free_tier_usage.write().await;

        let (hour, count) = usage.entry(client_id.to_string()).or_insert((current_hour, 0));

        if *hour != current_hour {
            *hour = current_hour;
            *count = 0;
        }

        *count += 1;
    }

    /// Check rate limit for a client.
    pub async fn check_rate_limit(&self, client_id: &str) -> bool {
        let now = chrono::Utc::now().timestamp() as u64;
        let current_minute = now / 60;

        let limits = self.rate_limits.read().await;

        match limits.get(client_id) {
            Some((minute, count)) if *minute == current_minute => {
                *count < self.config.rate_limit_per_minute
            }
            _ => true,
        }
    }

    /// Record a request for rate limiting.
    pub async fn record_request(&self, client_id: &str) {
        let now = chrono::Utc::now().timestamp() as u64;
        let current_minute = now / 60;

        let mut limits = self.rate_limits.write().await;

        let (minute, count) = limits.entry(client_id.to_string()).or_insert((current_minute, 0));

        if *minute != current_minute {
            *minute = current_minute;
            *count = 0;
        }

        *count += 1;
    }

    /// Get price for a request to a given path.
    pub fn get_price(&self, path: &str, method: &str) -> u64 {
        self.pricing.get_price(path, method)
            .unwrap_or(self.config.base_price_per_request)
    }
}

/// The HTTP 402 proxy server.
pub struct Proxy {
    state: Arc<ProxyState>,
}

impl Proxy {
    /// Create a new proxy server.
    pub fn new(state: Arc<ProxyState>) -> Self {
        Self { state }
    }

    /// Build the Axum router.
    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(Self::index))
            .route("/health", get(Self::health))
            .route("/pricing", get(Self::get_pricing))
            .route("/*path", any(Self::proxy_handler))
            .with_state(self.state.clone())
    }

    /// Index page with proxy info.
    async fn index(State(state): State<Arc<ProxyState>>) -> impl IntoResponse {
        let info = serde_json::json!({
            "service": "PeerClaw HTTP 402 Proxy",
            "version": env!("CARGO_PKG_VERSION"),
            "base_price": format!("{:.6} PCLAW", from_micro(state.config.base_price_per_request)),
            "payment_channels": state.config.enable_channels,
            "free_tier": state.config.free_tier_enabled,
            "free_requests_per_hour": state.config.free_tier_requests_per_hour,
        });

        axum::Json(info)
    }

    /// Health check endpoint.
    async fn health() -> impl IntoResponse {
        (StatusCode::OK, "OK")
    }

    /// Get pricing information.
    async fn get_pricing(State(state): State<Arc<ProxyState>>) -> impl IntoResponse {
        axum::Json(state.pricing.clone())
    }

    /// Main proxy handler.
    async fn proxy_handler(
        State(state): State<Arc<ProxyState>>,
        headers: HeaderMap,
        request: Request,
    ) -> Result<impl IntoResponse, ProxyError> {
        let path = request.uri().path().to_string();
        let method = request.method().as_str();

        // Extract client ID (from header or IP)
        let client_id = headers
            .get("X-Client-ID")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("anonymous")
            .to_string();

        // Check rate limit
        if !state.check_rate_limit(&client_id).await {
            return Err(ProxyError::RateLimited);
        }

        // Get price for this endpoint
        let price = state.get_price(&path, method);

        // Check for payment
        let payment_proof = PaymentProof::from_headers(&headers);

        let paid = match payment_proof {
            Some(proof) => {
                // Verify payment proof
                proof.verify(price).map_err(ProxyError::InvalidPayment)?
            }
            None => {
                // No payment provided, check free tier
                if state.check_free_tier(&client_id).await {
                    state.use_free_tier(&client_id).await;
                    true
                } else {
                    // Return 402 with payment instructions
                    return Err(ProxyError::PaymentRequired(format!(
                        "Payment required: {:.6} PCLAW. Include X-Payment-Proof header.",
                        from_micro(price)
                    )));
                }
            }
        };

        if !paid {
            return Err(ProxyError::PaymentRequired(
                "Payment verification failed".into(),
            ));
        }

        // Record request for rate limiting
        state.record_request(&client_id).await;

        // TODO: Forward request to upstream and return response
        // For now, return a placeholder response
        Ok((
            StatusCode::OK,
            [(
                "X-Payment-Received",
                format!("{:.6} PCLAW", from_micro(price)),
            )],
            "Request processed (upstream forwarding not yet implemented)",
        ))
    }

    /// Run the proxy server.
    pub async fn run(self) -> anyhow::Result<()> {
        let addr = self.state.config.listen_addr;
        let listener = tokio::net::TcpListener::bind(addr).await?;

        tracing::info!("HTTP 402 Proxy listening on {}", addr);

        axum::serve(listener, self.router()).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::identity::NodeIdentity;
    use crate::wallet::WalletConfig;
    use tempfile::tempdir;

    async fn setup_proxy() -> (Arc<ProxyState>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db = Database::open(&dir.path().join("test.redb")).unwrap();
        let identity = Arc::new(NodeIdentity::generate());
        let wallet = Arc::new(
            Wallet::new(identity, WalletConfig::default(), db).unwrap()
        );

        let state = Arc::new(ProxyState::new(wallet, None, ProxyConfig::default()));
        (state, dir)
    }

    #[tokio::test]
    async fn test_free_tier() {
        let (state, _dir) = setup_proxy().await;

        // First request should be free
        assert!(state.check_free_tier("client1").await);
        state.use_free_tier("client1").await;

        // Should still have free requests
        assert!(state.check_free_tier("client1").await);
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let (state, _dir) = setup_proxy().await;

        // Should be under rate limit initially
        assert!(state.check_rate_limit("client1").await);

        // Record many requests
        for _ in 0..state.config.rate_limit_per_minute {
            state.record_request("client1").await;
        }

        // Should now be rate limited
        assert!(!state.check_rate_limit("client1").await);
    }

    #[tokio::test]
    async fn test_pricing() {
        let (state, _dir) = setup_proxy().await;

        let price = state.get_price("/api/v1/chat", "POST");
        assert!(price > 0);
    }
}
