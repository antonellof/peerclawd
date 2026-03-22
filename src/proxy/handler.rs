//! Request handler for HTTP 402 proxy.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

/// Handler for proxying requests to upstream services.
#[derive(Debug, Clone)]
pub struct ProxyHandler {
    /// Upstream URL to forward requests to
    pub upstream_url: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Maximum request body size
    pub max_body_size: usize,
}

impl Default for ProxyHandler {
    fn default() -> Self {
        Self {
            upstream_url: String::new(),
            timeout_secs: 60,
            max_body_size: 10 * 1024 * 1024, // 10 MB
        }
    }
}

impl ProxyHandler {
    /// Create a new handler with upstream URL.
    pub fn new(upstream_url: &str) -> Self {
        Self {
            upstream_url: upstream_url.to_string(),
            ..Default::default()
        }
    }

    /// Set request timeout.
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set max body size.
    pub fn with_max_body_size(mut self, max_bytes: usize) -> Self {
        self.max_body_size = max_bytes;
        self
    }

    /// Check if handler has a configured upstream.
    pub fn has_upstream(&self) -> bool {
        !self.upstream_url.is_empty()
    }
}

/// Upstream response wrapper.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct UpstreamResponse {
    /// HTTP status code
    pub status: u16,
    /// Response headers (key-value pairs)
    pub headers: Vec<(String, String)>,
    /// Response body
    pub body: Vec<u8>,
    /// Time taken in milliseconds
    pub latency_ms: u64,
}

#[allow(dead_code)]
impl UpstreamResponse {
    /// Create a new upstream response.
    pub fn new(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
            latency_ms: 0,
        }
    }

    /// Check if response indicates success.
    pub fn is_success(&self) -> bool {
        self.status >= 200 && self.status < 300
    }

    /// Get status code as axum StatusCode.
    pub fn status_code(&self) -> StatusCode {
        StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_creation() {
        let handler = ProxyHandler::new("http://localhost:8080")
            .with_timeout(30)
            .with_max_body_size(1024 * 1024);

        assert_eq!(handler.upstream_url, "http://localhost:8080");
        assert_eq!(handler.timeout_secs, 30);
        assert_eq!(handler.max_body_size, 1024 * 1024);
        assert!(handler.has_upstream());
    }

    #[test]
    fn test_upstream_response() {
        let response = UpstreamResponse::new(200, b"OK".to_vec());

        assert!(response.is_success());
        assert_eq!(response.status_code(), StatusCode::OK);

        let error_response = UpstreamResponse::new(500, b"Error".to_vec());
        assert!(!error_response.is_success());
    }
}
