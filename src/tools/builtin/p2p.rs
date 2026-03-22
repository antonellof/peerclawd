//! P2P-native tools for distributed operations.
//!
//! These tools leverage the P2P network and token economy:
//! - Job submission to the network
//! - Job status tracking
//! - Peer discovery
//! - Wallet balance and transactions

use std::time::Instant;

use async_trait::async_trait;

use crate::tools::tool::{
    Tool, ToolContext, ToolError, ToolOutput, ToolDomain, ApprovalRequirement,
    require_str, optional_str, optional_i64, optional_bool,
};

/// Job submission tool - submit work to the P2P network.
pub struct JobSubmitTool {
    // In production: reference to JobManager and P2P network
}

impl JobSubmitTool {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for JobSubmitTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for JobSubmitTool {
    fn name(&self) -> &str {
        "job_submit"
    }

    fn description(&self) -> &str {
        "Submit a job to the P2P network for distributed execution. \
         Jobs are matched with peers who can fulfill them, with payment via PCLAW tokens. \
         Use for inference, computation, or other resource-intensive tasks."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "type": {
                    "type": "string",
                    "description": "Job type: inference, compute, storage, tool",
                    "enum": ["inference", "compute", "storage", "tool"]
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt for inference jobs"
                },
                "model": {
                    "type": "string",
                    "description": "Model to use for inference jobs"
                },
                "max_budget": {
                    "type": "number",
                    "description": "Maximum budget in PCLAW tokens"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Job timeout in seconds (default: 300)"
                },
                "prefer_local": {
                    "type": "boolean",
                    "description": "Prefer local execution if possible (default: true)"
                },
                "tool_name": {
                    "type": "string",
                    "description": "Tool name for tool-type jobs"
                },
                "tool_params": {
                    "type": "object",
                    "description": "Tool parameters for tool-type jobs"
                }
            },
            "required": ["type"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let job_type = require_str(&params, "type")?;
        let timeout_secs = optional_i64(&params, "timeout_secs", 300) as u64;
        let prefer_local = optional_bool(&params, "prefer_local", true);
        let max_budget = params.get("max_budget")
            .and_then(|v| v.as_f64())
            .unwrap_or(10.0);

        // Generate job ID
        let job_id = format!("job_{}", &uuid::Uuid::new_v4().to_string()[..8]);

        // Build job request based on type
        let job_details = match job_type {
            "inference" => {
                let prompt = require_str(&params, "prompt")?;
                let model = optional_str(&params, "model").unwrap_or("default");
                serde_json::json!({
                    "type": "inference",
                    "prompt": prompt,
                    "model": model,
                    "max_tokens": 1000,
                })
            }
            "tool" => {
                let tool_name = require_str(&params, "tool_name")?;
                let tool_params = params.get("tool_params").cloned().unwrap_or(serde_json::json!({}));
                serde_json::json!({
                    "type": "tool",
                    "tool_name": tool_name,
                    "tool_params": tool_params,
                })
            }
            "compute" | "storage" => {
                serde_json::json!({
                    "type": job_type,
                    "params": params.clone(),
                })
            }
            _ => return Err(ToolError::InvalidParameters(format!("Unknown job type: {}", job_type))),
        };

        // TODO: Actually submit to JobManager and P2P network
        // For now, return a placeholder response
        let result = serde_json::json!({
            "job_id": job_id,
            "status": "submitted",
            "type": job_type,
            "details": job_details,
            "max_budget": max_budget,
            "timeout_secs": timeout_secs,
            "prefer_local": prefer_local,
            "submitted_by": ctx.peer_id,
            "submitted_at": chrono::Utc::now().to_rfc3339(),
        });

        tracing::info!(
            job_id = %job_id,
            job_type = %job_type,
            "Job submitted to network"
        );

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::UnlessAutoApproved // Jobs cost tokens
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any // Can submit from anywhere
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Job status tool - check the status of submitted jobs.
pub struct JobStatusTool {
    // In production: reference to JobManager
}

impl JobStatusTool {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for JobStatusTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for JobStatusTool {
    fn name(&self) -> &str {
        "job_status"
    }

    fn description(&self) -> &str {
        "Check the status of a submitted job. Returns current status, \
         result if completed, or error if failed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "job_id": {
                    "type": "string",
                    "description": "Job ID to check"
                },
                "wait": {
                    "type": "boolean",
                    "description": "Wait for job completion (default: false)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Wait timeout in seconds (default: 30)"
                }
            },
            "required": ["job_id"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let job_id = require_str(&params, "job_id")?;
        let wait = optional_bool(&params, "wait", false);
        let timeout = optional_i64(&params, "timeout", 30) as u64;

        // TODO: Actually query JobManager
        // For now, return a placeholder response
        let result = serde_json::json!({
            "job_id": job_id,
            "status": "pending",
            "progress": 0,
            "result": null,
            "error": null,
            "executed_by": null,
            "tokens_spent": 0,
            "waited": wait,
            "timeout": timeout,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Peer discovery tool - find peers with specific capabilities.
pub struct PeerDiscoveryTool {
    // In production: reference to P2P Network
}

impl PeerDiscoveryTool {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for PeerDiscoveryTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for PeerDiscoveryTool {
    fn name(&self) -> &str {
        "peer_discovery"
    }

    fn description(&self) -> &str {
        "Discover peers on the P2P network. Find peers with specific \
         capabilities like GPU compute, storage, or specific models."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "capability": {
                    "type": "string",
                    "description": "Filter by capability: inference, compute, storage, any",
                    "enum": ["inference", "compute", "storage", "any"]
                },
                "model": {
                    "type": "string",
                    "description": "Filter by model availability"
                },
                "max_price": {
                    "type": "number",
                    "description": "Maximum price per unit in PCLAW"
                },
                "min_reliability": {
                    "type": "number",
                    "description": "Minimum reliability score (0-100)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum peers to return (default: 10)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let capability = optional_str(&params, "capability").unwrap_or("any");
        let limit = optional_i64(&params, "limit", 10) as usize;

        // TODO: Actually query P2P network
        // For now, return a placeholder response with the local peer
        let peers = vec![
            serde_json::json!({
                "peer_id": ctx.peer_id,
                "capabilities": ["inference", "compute"],
                "models": ["llama-3.2-3b", "qwen-2.5-7b"],
                "price_per_token": 0.001,
                "reliability": 100,
                "latency_ms": 0,
                "is_local": true,
            })
        ];

        let result = serde_json::json!({
            "peers": peers,
            "peer_count": peers.len(),
            "filter": {
                "capability": capability,
                "limit": limit,
            },
            "network_size": 1, // Total known peers
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Any
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Wallet balance tool - check PCLAW token balance.
pub struct WalletBalanceTool {
    // In production: reference to Wallet
}

impl WalletBalanceTool {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for WalletBalanceTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for WalletBalanceTool {
    fn name(&self) -> &str {
        "wallet_balance"
    }

    fn description(&self) -> &str {
        "Check your PCLAW token wallet balance. Shows available balance, \
         locked (in escrow), and total. Also shows recent transactions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "include_history": {
                    "type": "boolean",
                    "description": "Include recent transaction history (default: false)"
                },
                "history_limit": {
                    "type": "integer",
                    "description": "Number of transactions to include (default: 10)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let include_history = optional_bool(&params, "include_history", false);
        let _history_limit = optional_i64(&params, "history_limit", 10) as usize;

        // TODO: Actually query Wallet
        // For now, return placeholder data
        let mut result = serde_json::json!({
            "peer_id": ctx.peer_id,
            "balance": {
                "available": 15000.0,
                "locked": 0.0,
                "total": 15000.0,
                "unit": "PCLAW",
            },
            "stats": {
                "total_earned": 0.0,
                "total_spent": 0.0,
                "jobs_completed": 0,
                "jobs_submitted": 0,
            }
        });

        if include_history {
            result["transactions"] = serde_json::json!([
                // Empty for now
            ]);
        }

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local // Wallet is local
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_job_submit() {
        let tool = JobSubmitTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        let result = tool.execute(
            serde_json::json!({
                "type": "inference",
                "prompt": "Hello, world!",
                "model": "llama-3.2-3b"
            }),
            &ctx,
        ).await.unwrap();

        assert!(result.success);
        assert!(result.data["job_id"].as_str().is_some());
        assert_eq!(result.data["status"], "submitted");
    }

    #[tokio::test]
    async fn test_peer_discovery() {
        let tool = PeerDiscoveryTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        let result = tool.execute(
            serde_json::json!({
                "capability": "inference"
            }),
            &ctx,
        ).await.unwrap();

        assert!(result.success);
        assert!(result.data["peers"].as_array().is_some());
    }

    #[tokio::test]
    async fn test_wallet_balance() {
        let tool = WalletBalanceTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        let result = tool.execute(
            serde_json::json!({}),
            &ctx,
        ).await.unwrap();

        assert!(result.success);
        assert!(result.data["balance"]["available"].as_f64().is_some());
    }
}
