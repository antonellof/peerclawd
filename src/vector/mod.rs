//! Vector database module using vectX.
//!
//! Provides semantic vector search for memories, documents, and embeddings
//! using vectX's HNSW indexing and SIMD-optimized similarity search.

pub mod embeddings;

use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// Re-export vectX types for convenience
pub use vectx::{
    Collection, CollectionConfig, Distance, Filter, PayloadFilter, Point, PointId, Vector,
};

// Re-export embeddings
pub use embeddings::{Embedder, EmbeddingConfig, EmbeddingProvider, get_embedder, init_embedder};

/// Default embedding dimension (compatible with most sentence transformers)
pub const DEFAULT_EMBEDDING_DIM: usize = 384;

/// Error type for vector operations
#[derive(Debug, Error)]
pub enum VectorError {
    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    #[error("Collection already exists: {0}")]
    CollectionExists(String),

    #[error("Invalid vector dimension: expected {expected}, got {actual}")]
    InvalidDimension { expected: usize, actual: usize },

    #[error("vectX error: {0}")]
    VectxError(#[from] vectx::Error),

    #[error("Storage error: {0}")]
    StorageError(String),

    #[error("Embedding error: {0}")]
    EmbeddingError(String),
}

pub type Result<T> = std::result::Result<T, VectorError>;

/// Configuration for VectorStore
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorStoreConfig {
    /// Default embedding dimension
    pub embedding_dim: usize,
    /// Use HNSW index for large collections
    pub use_hnsw: bool,
    /// Enable BM25 text search
    pub enable_bm25: bool,
    /// Distance metric
    pub distance: DistanceMetric,
}

impl Default for VectorStoreConfig {
    fn default() -> Self {
        Self {
            embedding_dim: DEFAULT_EMBEDDING_DIM,
            use_hnsw: true,
            enable_bm25: true,
            distance: DistanceMetric::Cosine,
        }
    }
}

/// Distance metric for similarity search
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum DistanceMetric {
    #[default]
    Cosine,
    Euclidean,
    DotProduct,
}

impl From<DistanceMetric> for Distance {
    fn from(m: DistanceMetric) -> Self {
        match m {
            DistanceMetric::Cosine => Distance::Cosine,
            DistanceMetric::Euclidean => Distance::Euclidean,
            DistanceMetric::DotProduct => Distance::Dot,
        }
    }
}

/// Search result with score and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Point ID
    pub id: String,
    /// Similarity score (0.0 to 1.0 for cosine)
    pub score: f32,
    /// Optional payload/metadata
    pub payload: Option<serde_json::Value>,
    /// Original text (if stored)
    pub text: Option<String>,
}

impl SearchResult {
    /// Create from vectX Point and score
    fn from_point(point: &Point, score: f32) -> Self {
        let text = point
            .payload
            .as_ref()
            .and_then(|p| p.get("text"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Self {
            id: point.id.to_string(),
            score,
            payload: point.payload.clone(),
            text,
        }
    }
}

/// Vector store managing multiple collections
pub struct VectorStore {
    config: VectorStoreConfig,
    collections: Arc<RwLock<std::collections::HashMap<String, Arc<Collection>>>>,
    #[allow(dead_code)]
    storage_path: Option<std::path::PathBuf>,
}

impl VectorStore {
    /// Create a new in-memory vector store
    pub fn new(config: VectorStoreConfig) -> Self {
        Self {
            config,
            collections: Arc::new(RwLock::new(std::collections::HashMap::new())),
            storage_path: None,
        }
    }

    /// Create vector store with persistence
    pub fn with_storage(config: VectorStoreConfig, path: &Path) -> Result<Self> {
        // Create storage directory if it doesn't exist
        if !path.exists() {
            std::fs::create_dir_all(path)
                .map_err(|e| VectorError::StorageError(e.to_string()))?;
        }

        let store = Self {
            config,
            collections: Arc::new(RwLock::new(std::collections::HashMap::new())),
            storage_path: Some(path.to_path_buf()),
        };

        // Load existing collections
        store.load_collections()?;

        Ok(store)
    }

    /// Load collections from storage
    fn load_collections(&self) -> Result<()> {
        // TODO: Implement collection persistence using vectx-storage
        // For now, collections are ephemeral
        Ok(())
    }

    /// Create a new collection
    pub fn create_collection(&self, name: &str) -> Result<()> {
        self.create_collection_with_dim(name, self.config.embedding_dim)
    }

    /// Create a new collection with specific dimension
    pub fn create_collection_with_dim(&self, name: &str, dim: usize) -> Result<()> {
        let mut collections = self.collections.write();

        if collections.contains_key(name) {
            return Err(VectorError::CollectionExists(name.to_string()));
        }

        let config = CollectionConfig {
            name: name.to_string(),
            vector_dim: dim,
            distance: self.config.distance.into(),
            use_hnsw: self.config.use_hnsw,
            enable_bm25: self.config.enable_bm25,
        };

        let collection = Collection::new(config);
        collections.insert(name.to_string(), Arc::new(collection));

        tracing::info!(collection = name, dim = dim, "Created vector collection");
        Ok(())
    }

    /// Get or create a collection
    pub fn get_or_create_collection(&self, name: &str) -> Result<Arc<Collection>> {
        {
            let collections = self.collections.read();
            if let Some(col) = collections.get(name) {
                return Ok(col.clone());
            }
        }

        self.create_collection(name)?;
        let collections = self.collections.read();
        Ok(collections.get(name).unwrap().clone())
    }

    /// Delete a collection
    pub fn delete_collection(&self, name: &str) -> Result<bool> {
        let mut collections = self.collections.write();
        Ok(collections.remove(name).is_some())
    }

    /// List all collections
    pub fn list_collections(&self) -> Vec<CollectionInfo> {
        let collections = self.collections.read();
        collections
            .iter()
            .map(|(name, col)| CollectionInfo {
                name: name.clone(),
                count: col.count(),
                dimension: col.vector_dim(),
            })
            .collect()
    }

    /// Insert a vector with payload
    pub fn upsert(
        &self,
        collection: &str,
        id: &str,
        vector: Vec<f32>,
        payload: Option<serde_json::Value>,
    ) -> Result<()> {
        let col = self.get_collection(collection)?;

        // Validate dimension
        if vector.len() != col.vector_dim() {
            return Err(VectorError::InvalidDimension {
                expected: col.vector_dim(),
                actual: vector.len(),
            });
        }

        let point = Point::new(
            PointId::String(id.to_string()),
            Vector::new(vector),
            payload,
        );

        col.upsert(point)?;
        Ok(())
    }

    /// Insert text with automatic embedding (placeholder - requires embedding model)
    pub fn upsert_text(
        &self,
        collection: &str,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        extra_payload: Option<serde_json::Value>,
    ) -> Result<()> {
        let mut payload = serde_json::json!({
            "text": text,
        });

        if let Some(extra) = extra_payload {
            if let (Some(obj), Some(extra_obj)) =
                (payload.as_object_mut(), extra.as_object())
            {
                for (k, v) in extra_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }

        self.upsert(collection, id, embedding, Some(payload))
    }

    /// Search for similar vectors
    pub fn search(
        &self,
        collection: &str,
        query: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;
        let query_vec = Vector::new(query);

        let results = col.search(&query_vec, limit, None);

        Ok(results
            .into_iter()
            .map(|(point, score)| SearchResult::from_point(&point, score))
            .collect())
    }

    /// Search with filter
    pub fn search_with_filter(
        &self,
        collection: &str,
        query: Vec<f32>,
        limit: usize,
        filter: &PayloadFilter,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;
        let query_vec = Vector::new(query);

        let results = col.search(&query_vec, limit, Some(filter));

        Ok(results
            .into_iter()
            .map(|(point, score)| SearchResult::from_point(&point, score))
            .collect())
    }

    /// Full-text search (BM25)
    pub fn search_text(
        &self,
        collection: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;
        let results = col.search_text(query, limit);

        // Get full points for results
        Ok(results
            .into_iter()
            .filter_map(|(id, score)| {
                col.get(&id).map(|point| SearchResult::from_point(&point, score))
            })
            .collect())
    }

    /// Hybrid search combining vector and text
    pub fn hybrid_search(
        &self,
        collection: &str,
        query_vector: Vec<f32>,
        query_text: &str,
        limit: usize,
        vector_weight: f32,
    ) -> Result<Vec<SearchResult>> {
        let col = self.get_collection(collection)?;

        // Vector search
        let query_vec = Vector::new(query_vector);
        let vector_results = col.search(&query_vec, limit * 2, None);

        // Text search
        let text_results = col.search_text(query_text, limit * 2);

        // Combine results with weighted scoring
        let mut combined: std::collections::HashMap<String, f32> =
            std::collections::HashMap::new();

        let text_weight = 1.0 - vector_weight;

        for (point, score) in &vector_results {
            let id = point.id.to_string();
            *combined.entry(id).or_insert(0.0) += score * vector_weight;
        }

        for (id, score) in &text_results {
            *combined.entry(id.clone()).or_insert(0.0) += score * text_weight;
        }

        // Sort by combined score
        let mut results: Vec<_> = combined.into_iter().collect();
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Get full points
        Ok(results
            .into_iter()
            .take(limit)
            .filter_map(|(id, score)| {
                col.get(&id).map(|point| SearchResult::from_point(&point, score))
            })
            .collect())
    }

    /// Get a point by ID
    pub fn get(&self, collection: &str, id: &str) -> Result<Option<SearchResult>> {
        let col = self.get_collection(collection)?;
        Ok(col
            .get(id)
            .map(|point| SearchResult::from_point(&point, 1.0)))
    }

    /// Delete a point
    pub fn delete(&self, collection: &str, id: &str) -> Result<bool> {
        let col = self.get_collection(collection)?;
        col.delete(id)?;
        Ok(true)
    }

    /// Get collection count
    pub fn count(&self, collection: &str) -> Result<usize> {
        let col = self.get_collection(collection)?;
        Ok(col.count())
    }

    /// Get a collection by name
    fn get_collection(&self, name: &str) -> Result<Arc<Collection>> {
        let collections = self.collections.read();
        collections
            .get(name)
            .cloned()
            .ok_or_else(|| VectorError::CollectionNotFound(name.to_string()))
    }
}

/// Information about a collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionInfo {
    pub name: String,
    pub count: usize,
    pub dimension: usize,
}

/// Global vector store instance
static VECTOR_STORE: std::sync::LazyLock<RwLock<Option<Arc<VectorStore>>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// Initialize the global vector store
pub fn init_vector_store(config: VectorStoreConfig) {
    let mut store = VECTOR_STORE.write();
    *store = Some(Arc::new(VectorStore::new(config)));
}

/// Initialize the global vector store with persistence
pub fn init_vector_store_with_storage(
    config: VectorStoreConfig,
    path: &Path,
) -> Result<()> {
    let vector_store = VectorStore::with_storage(config, path)?;
    let mut store = VECTOR_STORE.write();
    *store = Some(Arc::new(vector_store));
    Ok(())
}

/// Get the global vector store
pub fn get_vector_store() -> Option<Arc<VectorStore>> {
    let store = VECTOR_STORE.read();
    store.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_collection() {
        let store = VectorStore::new(VectorStoreConfig::default());
        store.create_collection("test").unwrap();

        let collections = store.list_collections();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].name, "test");
    }

    #[test]
    fn test_upsert_and_search() {
        let store = VectorStore::new(VectorStoreConfig {
            embedding_dim: 4,
            ..Default::default()
        });
        store.create_collection_with_dim("test", 4).unwrap();

        // Insert vectors
        store
            .upsert(
                "test",
                "vec1",
                vec![1.0, 0.0, 0.0, 0.0],
                Some(serde_json::json!({"text": "hello"})),
            )
            .unwrap();
        store
            .upsert(
                "test",
                "vec2",
                vec![0.0, 1.0, 0.0, 0.0],
                Some(serde_json::json!({"text": "world"})),
            )
            .unwrap();
        store
            .upsert(
                "test",
                "vec3",
                vec![0.9, 0.1, 0.0, 0.0],
                Some(serde_json::json!({"text": "hi"})),
            )
            .unwrap();

        // Search for similar to vec1
        let results = store.search("test", vec![1.0, 0.0, 0.0, 0.0], 2).unwrap();

        assert_eq!(results.len(), 2);
        // vec1 should be most similar to itself
        assert_eq!(results[0].id, "vec1");
        // vec3 should be second (0.9 similarity)
        assert_eq!(results[1].id, "vec3");
    }

    #[test]
    fn test_text_search() {
        let store = VectorStore::new(VectorStoreConfig {
            embedding_dim: 4,
            enable_bm25: true,
            ..Default::default()
        });
        store.create_collection_with_dim("test", 4).unwrap();

        // Insert with text
        store
            .upsert_text(
                "test",
                "doc1",
                "The quick brown fox jumps over the lazy dog",
                vec![1.0, 0.0, 0.0, 0.0],
                None,
            )
            .unwrap();
        store
            .upsert_text(
                "test",
                "doc2",
                "A lazy cat sleeps on the couch",
                vec![0.0, 1.0, 0.0, 0.0],
                None,
            )
            .unwrap();

        // Search for "lazy"
        let results = store.search_text("test", "lazy", 10).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_delete() {
        let store = VectorStore::new(VectorStoreConfig {
            embedding_dim: 4,
            ..Default::default()
        });
        store.create_collection_with_dim("test", 4).unwrap();

        store
            .upsert("test", "vec1", vec![1.0, 0.0, 0.0, 0.0], None)
            .unwrap();

        assert_eq!(store.count("test").unwrap(), 1);

        store.delete("test", "vec1").unwrap();

        assert_eq!(store.count("test").unwrap(), 0);
    }
}
