//! Text embedding generation for vector search.
//!
//! Provides multiple embedding strategies:
//! - Simple word-based embeddings (fallback)
//! - Local model embeddings via candle (when available)
//! - Remote API embeddings (OpenAI-compatible)

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::DEFAULT_EMBEDDING_DIM;

#[derive(Debug, Error)]
pub enum EmbeddingError {
    #[error("Model not loaded")]
    ModelNotLoaded,

    #[error("Tokenization error: {0}")]
    TokenizationError(String),

    #[error("Inference error: {0}")]
    InferenceError(String),

    #[error("API error: {0}")]
    ApiError(String),

    #[error("Invalid text: {0}")]
    InvalidText(String),
}

pub type Result<T> = std::result::Result<T, EmbeddingError>;

/// Embedding provider type
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum EmbeddingProvider {
    /// Simple bag-of-words embeddings (no model required)
    #[default]
    Simple,
    /// Local model via candle/llama.cpp
    Local(String),
    /// Remote API (OpenAI-compatible)
    Remote {
        url: String,
        model: String,
        api_key: Option<String>,
    },
}

/// Configuration for embedding generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Embedding provider
    pub provider: EmbeddingProvider,
    /// Output dimension
    pub dimension: usize,
    /// Normalize embeddings
    pub normalize: bool,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: EmbeddingProvider::Simple,
            dimension: DEFAULT_EMBEDDING_DIM,
            normalize: true,
        }
    }
}

/// Embedding generator
pub struct Embedder {
    config: EmbeddingConfig,
    /// Vocabulary for simple embeddings
    vocab: Arc<RwLock<HashMap<String, usize>>>,
    /// IDF weights for TF-IDF style embeddings
    idf: Arc<RwLock<HashMap<String, f32>>>,
    /// Document count for IDF
    doc_count: Arc<RwLock<usize>>,
}

impl Embedder {
    /// Create a new embedder with the given config
    pub fn new(config: EmbeddingConfig) -> Self {
        Self {
            config,
            vocab: Arc::new(RwLock::new(HashMap::new())),
            idf: Arc::new(RwLock::new(HashMap::new())),
            doc_count: Arc::new(RwLock::new(0)),
        }
    }

    /// Create a simple embedder (no model required)
    pub fn simple() -> Self {
        Self::new(EmbeddingConfig::default())
    }

    /// Generate embedding for text
    pub async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        match &self.config.provider {
            EmbeddingProvider::Simple => self.embed_simple(text),
            EmbeddingProvider::Local(_model) => {
                // TODO: Use candle for local embedding model
                self.embed_simple(text)
            }
            EmbeddingProvider::Remote { url, model, api_key } => {
                self.embed_remote(text, url, model, api_key.as_deref()).await
            }
        }
    }

    /// Generate embeddings for multiple texts (batched)
    pub async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        // For now, just embed sequentially
        // TODO: Implement batching for efficiency
        let mut embeddings = Vec::with_capacity(texts.len());
        for text in texts {
            embeddings.push(self.embed(text).await?);
        }
        Ok(embeddings)
    }

    /// Simple bag-of-words with TF-IDF style weighting
    fn embed_simple(&self, text: &str) -> Result<Vec<f32>> {
        let tokens = self.tokenize(text);
        if tokens.is_empty() {
            return Err(EmbeddingError::InvalidText("Empty text".to_string()));
        }

        // Update vocabulary
        {
            let mut vocab = self.vocab.write();
            for token in &tokens {
                let next_idx = vocab.len();
                vocab.entry(token.clone()).or_insert(next_idx);
            }
        }

        // Generate embedding using hash-based projection
        let mut embedding = vec![0.0f32; self.config.dimension];

        // Count term frequencies
        let mut tf: HashMap<&str, f32> = HashMap::new();
        for token in &tokens {
            *tf.entry(token).or_insert(0.0) += 1.0;
        }

        // Normalize TF
        let max_tf = tf.values().cloned().fold(1.0f32, f32::max);
        for freq in tf.values_mut() {
            *freq = 0.5 + 0.5 * (*freq / max_tf);
        }

        // Project tokens to embedding space using hashing
        for (token, freq) in &tf {
            // Use multiple hash functions for better coverage
            let hash1 = self.hash_token(token, 0);
            let hash2 = self.hash_token(token, 1);
            let hash3 = self.hash_token(token, 2);

            let idx1 = hash1 % self.config.dimension;
            let idx2 = hash2 % self.config.dimension;
            let idx3 = hash3 % self.config.dimension;

            // Sign determined by hash
            let sign1 = if (hash1 / self.config.dimension) % 2 == 0 { 1.0 } else { -1.0 };
            let sign2 = if (hash2 / self.config.dimension) % 2 == 0 { 1.0 } else { -1.0 };
            let sign3 = if (hash3 / self.config.dimension) % 2 == 0 { 1.0 } else { -1.0 };

            embedding[idx1] += sign1 * freq;
            embedding[idx2] += sign2 * freq * 0.5;
            embedding[idx3] += sign3 * freq * 0.25;
        }

        // Normalize if configured
        if self.config.normalize {
            self.normalize_embedding(&mut embedding);
        }

        Ok(embedding)
    }

    /// Remote API embedding (OpenAI-compatible)
    async fn embed_remote(
        &self,
        text: &str,
        url: &str,
        model: &str,
        api_key: Option<&str>,
    ) -> Result<Vec<f32>> {
        let client = reqwest::Client::new();

        let mut request = client
            .post(format!("{}/embeddings", url.trim_end_matches('/')))
            .json(&serde_json::json!({
                "model": model,
                "input": text,
            }));

        if let Some(key) = api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        let response = request
            .send()
            .await
            .map_err(|e| EmbeddingError::ApiError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(EmbeddingError::ApiError(format!(
                "API returned {}: {}",
                status, text
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| EmbeddingError::ApiError(e.to_string()))?;

        // Extract embedding from response
        let embedding = json["data"][0]["embedding"]
            .as_array()
            .ok_or_else(|| EmbeddingError::ApiError("Invalid response format".to_string()))?
            .iter()
            .filter_map(|v| v.as_f64().map(|f| f as f32))
            .collect();

        Ok(embedding)
    }

    /// Tokenize text into words
    fn tokenize(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|s| s.len() > 1)
            .map(|s| s.to_string())
            .collect()
    }

    /// Hash a token to a bucket index
    fn hash_token(&self, token: &str, seed: usize) -> usize {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        seed.hash(&mut hasher);
        token.hash(&mut hasher);
        hasher.finish() as usize
    }

    /// Normalize embedding to unit length
    fn normalize_embedding(&self, embedding: &mut [f32]) {
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in embedding.iter_mut() {
                *x /= norm;
            }
        }
    }

    /// Update IDF weights from a corpus
    pub fn update_idf(&self, documents: &[&str]) {
        let mut idf = self.idf.write();
        let mut doc_count = self.doc_count.write();

        for doc in documents {
            *doc_count += 1;
            let tokens: std::collections::HashSet<_> = self.tokenize(doc).into_iter().collect();
            for token in tokens {
                *idf.entry(token).or_insert(0.0) += 1.0;
            }
        }

        // Convert to IDF
        let n = *doc_count as f32;
        for freq in idf.values_mut() {
            *freq = (n / (*freq + 1.0)).ln() + 1.0;
        }
    }

    /// Compute similarity between two embeddings
    pub fn similarity(&self, a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() {
            return 0.0;
        }

        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();

        if self.config.normalize {
            // If normalized, dot product = cosine similarity
            dot
        } else {
            let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
            let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm_a > 0.0 && norm_b > 0.0 {
                dot / (norm_a * norm_b)
            } else {
                0.0
            }
        }
    }
}

/// Global embedder instance
static EMBEDDER: std::sync::LazyLock<RwLock<Option<Arc<Embedder>>>> =
    std::sync::LazyLock::new(|| RwLock::new(None));

/// Initialize the global embedder
pub fn init_embedder(config: EmbeddingConfig) {
    let mut embedder = EMBEDDER.write();
    *embedder = Some(Arc::new(Embedder::new(config)));
}

/// Get the global embedder (creates simple embedder if not initialized)
pub fn get_embedder() -> Arc<Embedder> {
    {
        let embedder = EMBEDDER.read();
        if let Some(e) = &*embedder {
            return e.clone();
        }
    }

    // Initialize with simple embedder
    let mut embedder = EMBEDDER.write();
    if embedder.is_none() {
        *embedder = Some(Arc::new(Embedder::simple()));
    }
    embedder.as_ref().unwrap().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simple_embedding() {
        let embedder = Embedder::simple();
        let embedding = embedder.embed("Hello world, this is a test").await.unwrap();

        assert_eq!(embedding.len(), DEFAULT_EMBEDDING_DIM);

        // Check normalization
        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_similarity() {
        let embedder = Embedder::simple();

        let e1 = embedder.embed("The quick brown fox").await.unwrap();
        let e2 = embedder.embed("A fast brown fox").await.unwrap();
        let e3 = embedder.embed("Machine learning algorithms").await.unwrap();

        let sim_12 = embedder.similarity(&e1, &e2);
        let sim_13 = embedder.similarity(&e1, &e3);

        // Similar texts should have higher similarity
        assert!(sim_12 > sim_13);
    }

    #[tokio::test]
    async fn test_batch_embedding() {
        let embedder = Embedder::simple();
        let texts = vec!["First text", "Second text", "Third text"];
        let embeddings = embedder.embed_batch(&texts).await.unwrap();

        assert_eq!(embeddings.len(), 3);
        for e in &embeddings {
            assert_eq!(e.len(), DEFAULT_EMBEDDING_DIM);
        }
    }
}
