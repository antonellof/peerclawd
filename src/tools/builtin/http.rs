//! HTTP tools: http request and web fetch.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::Client;

use crate::tools::tool::{
    Tool, ToolContext, ToolError, ToolOutput, ToolDomain, ApprovalRequirement,
    require_str, optional_str, optional_i64,
};

/// Default timeout for HTTP requests.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum response size (5 MB).
const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024;

/// HTTP request tool.
pub struct HttpTool {
    client: Client,
}

impl HttpTool {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .user_agent("PeerClaw/0.2")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }
}

impl Default for HttpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for HttpTool {
    fn name(&self) -> &str {
        "http"
    }

    fn description(&self) -> &str {
        "Make HTTP requests (GET, POST, PUT, DELETE, PATCH). \
         Returns status code, headers, and body."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to request"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method (default: GET)",
                    "enum": ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS"]
                },
                "headers": {
                    "type": "object",
                    "description": "Request headers as key-value pairs"
                },
                "body": {
                    "type": "string",
                    "description": "Request body (for POST, PUT, PATCH)"
                },
                "json": {
                    "type": "object",
                    "description": "JSON body (auto-sets Content-Type)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 30)"
                },
                "follow_redirects": {
                    "type": "boolean",
                    "description": "Follow HTTP redirects (default: true)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let url = require_str(&params, "url")?;
        let method = optional_str(&params, "method").unwrap_or("GET").to_uppercase();
        let timeout_secs = optional_i64(&params, "timeout", 30) as u64;

        // Validate URL
        let parsed_url = reqwest::Url::parse(url)
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid URL: {}", e)))?;

        // Security: block local addresses
        if is_local_address(&parsed_url) {
            return Err(ToolError::NotAuthorized(
                "Requests to local addresses are not allowed".to_string()
            ));
        }

        // Build request
        let mut builder = match method.as_str() {
            "GET" => self.client.get(url),
            "POST" => self.client.post(url),
            "PUT" => self.client.put(url),
            "DELETE" => self.client.delete(url),
            "PATCH" => self.client.patch(url),
            "HEAD" => self.client.head(url),
            "OPTIONS" => self.client.request(reqwest::Method::OPTIONS, url),
            _ => return Err(ToolError::InvalidParameters(format!("Unknown method: {}", method))),
        };

        builder = builder.timeout(Duration::from_secs(timeout_secs));

        // Add headers
        if let Some(headers) = params.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(value_str) = value.as_str() {
                    builder = builder.header(key, value_str);
                }
            }
        }

        // Add body
        if let Some(json) = params.get("json") {
            builder = builder.json(json);
        } else if let Some(body) = params.get("body").and_then(|v| v.as_str()) {
            builder = builder.body(body.to_string());
        }

        // Execute request
        let response = builder.send().await
            .map_err(|e| ToolError::ExternalService(format!("Request failed: {}", e)))?;

        let status = response.status().as_u16();
        let status_text = response.status().canonical_reason().unwrap_or("Unknown");

        // Collect headers
        let headers: HashMap<String, String> = response.headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        // Get body (with size limit)
        let content_length = response.content_length().unwrap_or(0) as usize;
        if content_length > MAX_RESPONSE_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "Response too large: {} bytes (max {} bytes)",
                content_length, MAX_RESPONSE_SIZE
            )));
        }

        let body_bytes = response.bytes().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read body: {}", e)))?;

        // Try to parse as text, fall back to base64
        let body = if let Ok(text) = std::str::from_utf8(&body_bytes) {
            serde_json::json!({
                "type": "text",
                "content": text
            })
        } else {
            use base64::Engine;
            serde_json::json!({
                "type": "binary",
                "content": base64::engine::general_purpose::STANDARD.encode(&body_bytes),
                "size": body_bytes.len()
            })
        };

        let result = serde_json::json!({
            "url": url,
            "method": method,
            "status": status,
            "status_text": status_text,
            "headers": headers,
            "body": body,
            "elapsed_ms": start.elapsed().as_millis(),
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any // Can run anywhere
    }

    fn requires_sanitization(&self) -> bool {
        true // External content
    }

    fn rate_limit(&self) -> Option<u32> {
        Some(60) // 60 requests per minute
    }
}

/// Web fetch tool - fetches a URL and converts HTML to readable text.
pub struct WebFetchTool {
    client: Client,
}

impl WebFetchTool {
    pub fn new() -> Self {
        let client = Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .user_agent("Mozilla/5.0 (compatible; PeerClaw/0.2)")
            .build()
            .unwrap_or_else(|_| Client::new());

        Self { client }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch a web page and extract its text content. \
         Converts HTML to readable text, stripping scripts and styles."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector to extract specific content (optional)"
                },
                "max_length": {
                    "type": "integer",
                    "description": "Maximum text length to return (default: 50000)"
                },
                "include_links": {
                    "type": "boolean",
                    "description": "Include links in output (default: false)"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let url = require_str(&params, "url")?;
        let max_length = optional_i64(&params, "max_length", 50000) as usize;

        // Validate and fetch URL
        let parsed_url = reqwest::Url::parse(url)
            .map_err(|e| ToolError::InvalidParameters(format!("Invalid URL: {}", e)))?;

        if is_local_address(&parsed_url) {
            return Err(ToolError::NotAuthorized(
                "Requests to local addresses are not allowed".to_string()
            ));
        }

        let response = self.client.get(url).send().await
            .map_err(|e| ToolError::ExternalService(format!("Fetch failed: {}", e)))?;

        let status = response.status().as_u16();
        if !response.status().is_success() {
            return Err(ToolError::ExternalService(format!(
                "HTTP {}: {}",
                status,
                response.status().canonical_reason().unwrap_or("Error")
            )));
        }

        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();

        let html = response.text().await
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read response: {}", e)))?;

        // Extract text from HTML (simple approach)
        let text = html_to_text(&html, max_length);
        let title = extract_title(&html);

        let result = serde_json::json!({
            "url": url,
            "title": title,
            "content_type": content_type,
            "text": text,
            "text_length": text.len(),
            "truncated": text.len() >= max_length,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    fn requires_sanitization(&self) -> bool {
        true
    }

    fn rate_limit(&self) -> Option<u32> {
        Some(30) // 30 requests per minute
    }
}

fn is_local_address(url: &reqwest::Url) -> bool {
    if let Some(host) = url.host_str() {
        let host_lower = host.to_lowercase();
        host_lower == "localhost"
            || host_lower == "127.0.0.1"
            || host_lower == "::1"
            || host_lower.starts_with("192.168.")
            || host_lower.starts_with("10.")
            || host_lower.starts_with("172.16.")
            || host_lower.ends_with(".local")
    } else {
        false
    }
}

fn html_to_text(html: &str, max_length: usize) -> String {
    // Simple HTML to text conversion (production would use proper parser)
    let mut text = html.to_string();

    // Remove script and style content
    let script_re = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    text = script_re.replace_all(&text, "").to_string();

    let style_re = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    text = style_re.replace_all(&text, "").to_string();

    // Remove HTML comments
    let comment_re = regex::Regex::new(r"(?s)<!--.*?-->").unwrap();
    text = comment_re.replace_all(&text, "").to_string();

    // Convert common tags to text
    text = text.replace("<br>", "\n");
    text = text.replace("<br/>", "\n");
    text = text.replace("<br />", "\n");
    text = text.replace("</p>", "\n\n");
    text = text.replace("</div>", "\n");
    text = text.replace("</li>", "\n");
    text = text.replace("</h1>", "\n\n");
    text = text.replace("</h2>", "\n\n");
    text = text.replace("</h3>", "\n\n");

    // Remove all remaining HTML tags
    let tag_re = regex::Regex::new(r"<[^>]+>").unwrap();
    text = tag_re.replace_all(&text, "").to_string();

    // Decode HTML entities
    text = text.replace("&nbsp;", " ");
    text = text.replace("&amp;", "&");
    text = text.replace("&lt;", "<");
    text = text.replace("&gt;", ">");
    text = text.replace("&quot;", "\"");
    text = text.replace("&#39;", "'");

    // Clean up whitespace
    let ws_re = regex::Regex::new(r"\s+").unwrap();
    text = ws_re.replace_all(&text, " ").to_string();

    let nl_re = regex::Regex::new(r"\n{3,}").unwrap();
    text = nl_re.replace_all(&text, "\n\n").to_string();

    text = text.trim().to_string();

    // Truncate if needed
    if text.len() > max_length {
        text.truncate(max_length);
        // Find last complete word
        if let Some(pos) = text.rfind(' ') {
            text.truncate(pos);
        }
        text.push_str("...");
    }

    text
}

fn extract_title(html: &str) -> Option<String> {
    let title_re = regex::Regex::new(r"(?i)<title[^>]*>([^<]+)</title>").ok()?;
    title_re.captures(html)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_to_text() {
        let html = "<html><head><title>Test</title></head><body><p>Hello <b>World</b>!</p></body></html>";
        let text = html_to_text(html, 1000);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_extract_title() {
        let html = "<html><head><title>My Page Title</title></head></html>";
        assert_eq!(extract_title(html), Some("My Page Title".to_string()));
    }

    #[test]
    fn test_is_local_address() {
        let url = reqwest::Url::parse("http://localhost:8080").unwrap();
        assert!(is_local_address(&url));

        let url = reqwest::Url::parse("http://192.168.1.1").unwrap();
        assert!(is_local_address(&url));

        let url = reqwest::Url::parse("https://example.com").unwrap();
        assert!(!is_local_address(&url));
    }
}
