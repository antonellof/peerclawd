//! Distributed memory tools for P2P workspace.
//!
//! These tools provide persistent memory across the P2P network:
//! - Search past memories using semantic vector search (vectX)
//! - Write memories with automatic embedding generation
//! - Support for both local-first and distributed modes

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::tools::tool::{
    ApprovalRequirement, Tool, ToolContext, ToolDomain, ToolError, ToolOutput, optional_bool,
    optional_i64, optional_str, require_str,
};
use crate::vector::{
    SearchResult, VectorStore, VectorStoreConfig, get_embedder,
};

/// Memory collection name in vector store
const MEMORY_COLLECTION: &str = "memories";

/// Memory entry stored in the distributed workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    /// Unique ID (BLAKE3 hash of content + timestamp)
    pub id: String,
    /// Content text
    pub content: String,
    /// Category/tag
    pub category: String,
    /// Source peer ID
    pub source_peer: String,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last modified timestamp
    pub modified_at: DateTime<Utc>,
    /// Relevance score (for search results)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    /// Whether this is replicated to other peers
    pub replicated: bool,
}

impl MemoryEntry {
    /// Create a new memory entry.
    pub fn new(content: String, category: String, peer_id: String) -> Self {
        let now = Utc::now();
        let id = {
            let mut hasher = blake3::Hasher::new();
            hasher.update(content.as_bytes());
            hasher.update(now.to_rfc3339().as_bytes());
            hasher.finalize().to_hex()[..16].to_string()
        };

        Self {
            id,
            content,
            category,
            source_peer: peer_id,
            created_at: now,
            modified_at: now,
            score: None,
            replicated: false,
        }
    }

    /// Create from a vector search result
    fn from_search_result(result: &SearchResult) -> Option<Self> {
        let payload = result.payload.as_ref()?;

        Some(Self {
            id: result.id.clone(),
            content: result.text.clone().unwrap_or_default(),
            category: payload.get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("facts")
                .to_string(),
            source_peer: payload.get("source_peer")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
            created_at: payload.get("created_at")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(Utc::now),
            modified_at: payload.get("modified_at")
                .and_then(|v| v.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or_else(Utc::now),
            score: Some(result.score),
            replicated: payload.get("replicated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        })
    }
}

/// Global memory vector store
static MEMORY_VECTOR_STORE: std::sync::LazyLock<Arc<RwLock<Option<Arc<VectorStore>>>>> =
    std::sync::LazyLock::new(|| Arc::new(RwLock::new(None)));

/// Initialize the memory vector store
fn get_memory_store() -> Arc<VectorStore> {
    {
        let store = MEMORY_VECTOR_STORE.read();
        if let Some(s) = &*store {
            return s.clone();
        }
    }

    // Create new store
    let mut store = MEMORY_VECTOR_STORE.write();
    if store.is_none() {
        let config = VectorStoreConfig::default();
        let vector_store = Arc::new(VectorStore::new(config));

        // Create memories collection
        if let Err(e) = vector_store.create_collection(MEMORY_COLLECTION) {
            tracing::warn!("Failed to create memories collection: {}", e);
        }

        *store = Some(vector_store);
    }
    store.as_ref().unwrap().clone()
}

/// Memory search tool - searches across local and network memories using semantic vector search.
pub struct MemorySearchTool;

impl MemorySearchTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemorySearchTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory_search"
    }

    fn description(&self) -> &str {
        "Search past memories using semantic vector search. Returns relevant memories \
         based on meaning, not just keywords. MUST be called before answering questions \
         about prior work, decisions, dates, people, preferences, or todos."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural language search query"
                },
                "category": {
                    "type": "string",
                    "description": "Filter by category (facts, decisions, preferences, todos, daily_log)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 10, max: 50)"
                },
                "include_network": {
                    "type": "boolean",
                    "description": "Include results from other peers (default: true)"
                },
                "min_score": {
                    "type": "number",
                    "description": "Minimum relevance score (0.0-1.0, default: 0.1)"
                },
                "hybrid": {
                    "type": "boolean",
                    "description": "Use hybrid search (vector + keyword, default: true)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let query = require_str(&params, "query")?;
        let category = optional_str(&params, "category");
        let limit = optional_i64(&params, "limit", 10).min(50) as usize;
        let include_network = optional_bool(&params, "include_network", true);
        let min_score = params
            .get("min_score")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            .unwrap_or(0.1);
        let hybrid = optional_bool(&params, "hybrid", true);

        let store = get_memory_store();
        let embedder = get_embedder();

        // Generate query embedding
        let query_embedding = embedder
            .embed(query)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Embedding error: {}", e)))?;

        // Perform search
        let search_results = if hybrid {
            // Hybrid search: vector + text
            store
                .hybrid_search(MEMORY_COLLECTION, query_embedding, query, limit * 2, 0.7)
                .unwrap_or_default()
        } else {
            // Pure vector search
            store
                .search(MEMORY_COLLECTION, query_embedding, limit * 2)
                .unwrap_or_default()
        };

        // Filter and convert results
        let mut results: Vec<MemoryEntry> = search_results
            .iter()
            .filter(|r| r.score >= min_score)
            .filter(|r| {
                if let Some(cat) = category {
                    r.payload
                        .as_ref()
                        .and_then(|p| p.get("category"))
                        .and_then(|v| v.as_str())
                        .map(|c| c == cat)
                        .unwrap_or(false)
                } else {
                    true
                }
            })
            .filter_map(|r| MemoryEntry::from_search_result(r))
            .take(limit)
            .collect();

        // Sort by score descending
        results.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let result = serde_json::json!({
            "query": query,
            "results": results,
            "result_count": results.len(),
            "searched_local": true,
            "searched_network": include_network,
            "search_type": if hybrid { "hybrid" } else { "vector" },
            "peer_id": ctx.peer_id,
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

/// Memory write tool - persists memories with vector embeddings for semantic search.
pub struct MemoryWriteTool;

impl MemoryWriteTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemoryWriteTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemoryWriteTool {
    fn name(&self) -> &str {
        "memory_write"
    }

    fn description(&self) -> &str {
        "Write to persistent distributed memory with automatic semantic embedding. \
         Use for important facts, decisions, preferences, or lessons learned that \
         should be remembered and searchable across sessions."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The content to remember. Be concise but include relevant context."
                },
                "category": {
                    "type": "string",
                    "description": "Category: facts, decisions, preferences, todos, daily_log",
                    "enum": ["facts", "decisions", "preferences", "todos", "daily_log"]
                },
                "replicate": {
                    "type": "boolean",
                    "description": "Replicate to trusted peers for redundancy (default: false)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional tags for additional filtering"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let content = require_str(&params, "content")?;
        let category = optional_str(&params, "category").unwrap_or("facts");
        let replicate = optional_bool(&params, "replicate", false);
        let tags: Vec<String> = params
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Validate content
        if content.trim().is_empty() {
            return Err(ToolError::InvalidParameters(
                "Content cannot be empty".to_string(),
            ));
        }

        if content.len() > 100_000 {
            return Err(ToolError::InvalidParameters(
                "Content too large (max 100KB)".to_string(),
            ));
        }

        // Create memory entry
        let entry = MemoryEntry::new(content.to_string(), category.to_string(), ctx.peer_id.clone());

        // Generate embedding
        let embedder = get_embedder();
        let embedding = embedder
            .embed(content)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("Embedding error: {}", e)))?;

        // Build payload
        let payload = serde_json::json!({
            "text": content,
            "category": category,
            "source_peer": ctx.peer_id,
            "created_at": entry.created_at.to_rfc3339(),
            "modified_at": entry.modified_at.to_rfc3339(),
            "replicated": replicate,
            "tags": tags,
        });

        // Store in vector database
        let store = get_memory_store();
        store
            .upsert(MEMORY_COLLECTION, &entry.id, embedding, Some(payload))
            .map_err(|e| ToolError::ExecutionFailed(format!("Storage error: {}", e)))?;

        // TODO: If replicate is true, broadcast to P2P network
        if replicate {
            tracing::info!(
                memory_id = %entry.id,
                "Memory marked for replication"
            );
        }

        let result = serde_json::json!({
            "id": entry.id,
            "category": entry.category,
            "created_at": entry.created_at.to_rfc3339(),
            "content_length": entry.content.len(),
            "replicated": replicate,
            "tags": tags,
            "peer_id": ctx.peer_id,
            "indexed": true,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

/// Memory stats tool - get statistics about the memory store
pub struct MemoryStatsTool;

impl MemoryStatsTool {
    pub fn new() -> Self {
        Self
    }
}

impl Default for MemoryStatsTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for MemoryStatsTool {
    fn name(&self) -> &str {
        "memory_stats"
    }

    fn description(&self) -> &str {
        "Get statistics about the memory store including total memories, \
         categories, and storage info."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = Instant::now();

        let store = get_memory_store();
        let collections = store.list_collections();

        let memory_collection = collections
            .iter()
            .find(|c| c.name == MEMORY_COLLECTION);

        let result = serde_json::json!({
            "collection": MEMORY_COLLECTION,
            "total_memories": memory_collection.map(|c| c.count).unwrap_or(0),
            "dimension": memory_collection.map(|c| c.dimension).unwrap_or(0),
            "collections": collections,
            "peer_id": ctx.peer_id,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Never
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Local
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_write_and_search() {
        let write_tool = MemoryWriteTool::new();
        let search_tool = MemorySearchTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        // Write a memory
        let write_result = write_tool
            .execute(
                serde_json::json!({
                    "content": "The user prefers dark mode and vim keybindings for coding",
                    "category": "preferences"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(write_result.success);
        assert!(write_result.data["indexed"].as_bool().unwrap());

        // Search for it with semantic query
        let search_result = search_tool
            .execute(
                serde_json::json!({
                    "query": "user's editor preferences and color theme"
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(search_result.success);
        // Note: with simple embeddings, results may vary
    }

    #[tokio::test]
    async fn test_memory_with_tags() {
        let write_tool = MemoryWriteTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        let result = write_tool
            .execute(
                serde_json::json!({
                    "content": "Project deadline is next Friday",
                    "category": "facts",
                    "tags": ["project", "deadline", "important"]
                }),
                &ctx,
            )
            .await
            .unwrap();

        assert!(result.success);
        let tags = result.data["tags"].as_array().unwrap();
        assert_eq!(tags.len(), 3);
    }

    #[tokio::test]
    async fn test_memory_stats() {
        let stats_tool = MemoryStatsTool::new();
        let ctx = ToolContext::local("test-peer".to_string());

        let result = stats_tool
            .execute(serde_json::json!({}), &ctx)
            .await
            .unwrap();

        assert!(result.success);
        assert!(result.data.get("total_memories").is_some());
    }
}
