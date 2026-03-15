//! AI inference engine with GGUF model support.
//!
//! This module provides local inference capabilities for LLM models
//! in GGUF format, with automatic model caching and resource management.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │ InferenceEngine │
//! └────────┬────────┘
//!          │
//!    ┌─────┴─────┐
//!    │           │
//!    ▼           ▼
//! ┌──────┐  ┌──────────┐
//! │ModelCache│  │ModelRegistry│
//! └──────┘  └──────────┘
//! ```

pub mod batch;
pub mod cache;
pub mod distribution;
pub mod gguf;
pub mod model;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::RwLock;

pub use batch::{
    BatchAggregator, BatchConfig, BatchError, BatchInferenceExecutor,
    BatchProcessor, BatchRequest, BatchResponse, BatchStats,
};
pub use cache::{CacheError, LoadedModel, ModelCache, ModelHandle};
pub use distribution::{
    DistributionError, DownloadProgress, ModelAnnouncement, ModelDistributor,
    ModelDistributionMessage, ModelMetadata, CHUNK_SIZE,
};
pub use gguf::{AsyncGgufEngine, GgufBackend, GgufConfig, GgufEngine, GgufError, GgufModelHandle, GgufModelInfo};
pub use model::{ModelArchitecture, ModelId, ModelInfo, ModelRequirements, Quantization};

/// Inference engine for running LLM models.
pub struct InferenceEngine {
    /// Model cache for loaded models
    cache: ModelCache,
    /// Directory where models are stored
    models_dir: PathBuf,
    /// Engine configuration
    config: InferenceConfig,
    /// Model registry (available but not necessarily loaded)
    registry: Arc<RwLock<ModelRegistry>>,
}

impl InferenceEngine {
    /// Create a new inference engine.
    pub fn new(config: InferenceConfig) -> std::io::Result<Self> {
        // Ensure models directory exists
        std::fs::create_dir_all(&config.models_dir)?;

        let cache = ModelCache::new(config.max_loaded_models, config.max_memory_mb);
        let registry = Arc::new(RwLock::new(ModelRegistry::new()));

        let engine = Self {
            cache,
            models_dir: config.models_dir.clone(),
            config,
            registry,
        };

        Ok(engine)
    }

    /// Scan models directory and update registry.
    pub async fn scan_models(&self) -> std::io::Result<usize> {
        let mut count = 0;
        let mut registry = self.registry.write().await;

        for entry in std::fs::read_dir(&self.models_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().map_or(false, |e| e == "gguf") {
                if let Ok(info) = ModelInfo::from_path(path) {
                    registry.register(info);
                    count += 1;
                }
            }
        }

        tracing::info!(count, "Scanned models directory");
        Ok(count)
    }

    /// Get list of available models.
    pub async fn available_models(&self) -> Vec<ModelInfo> {
        self.registry.read().await.list()
    }

    /// Get list of loaded models.
    pub async fn loaded_models(&self) -> Vec<ModelId> {
        self.cache.loaded_models().await
    }

    /// Check if a model is available (in registry).
    pub async fn has_model(&self, model_id: &str) -> bool {
        self.registry.read().await.get(model_id).is_some()
    }

    /// Check if a model is loaded (in cache).
    pub async fn is_loaded(&self, model_id: &str) -> bool {
        self.cache.is_loaded(model_id).await
    }

    /// Load a model into memory.
    pub async fn load_model(&self, model_id: &str) -> Result<(), InferenceError> {
        // Check if already loaded
        if self.cache.is_loaded(model_id).await {
            return Ok(());
        }

        // Get model info from registry
        let info = self
            .registry
            .read()
            .await
            .get(model_id)
            .cloned()
            .ok_or_else(|| InferenceError::ModelNotFound(model_id.to_string()))?;

        tracing::info!(
            model_id = %model_id,
            size_mb = info.estimate_ram_mb(),
            "Loading model"
        );

        // TODO: Actually load the model using llama.cpp
        // For now, create a placeholder LoadedModel
        let loaded = LoadedModel {
            info,
            loaded_at: Instant::now(),
            last_used: Instant::now(),
            ref_count: 0,
            handle: ModelHandle::Placeholder,
        };

        self.cache
            .insert(loaded)
            .await
            .map_err(|e| InferenceError::CacheError(e.to_string()))?;

        tracing::info!(model_id = %model_id, "Model loaded");
        Ok(())
    }

    /// Unload a model from memory.
    pub async fn unload_model(&self, model_id: &str) -> Result<(), InferenceError> {
        // The cache handles this via LRU eviction
        // For explicit unload, we'd need to add a remove method to cache
        tracing::info!(model_id = %model_id, "Model unload requested");
        Ok(())
    }

    /// Run inference on a model.
    pub async fn generate(
        &self,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, InferenceError> {
        let start = Instant::now();

        // Ensure model is loaded
        if !self.cache.is_loaded(&request.model).await {
            self.load_model(&request.model).await?;
        }

        // Get model handle
        let model = self
            .cache
            .get(&request.model)
            .await
            .ok_or_else(|| InferenceError::ModelNotFound(request.model.clone()))?;

        // Track that we're using the model
        let _guard = ModelUseGuard::new(&self.cache, &request.model);

        let model_guard = model.read().await;

        // TODO: Implement actual inference using llama.cpp
        // For now, return a placeholder response
        tracing::info!(
            model = %request.model,
            prompt_len = request.prompt.len(),
            max_tokens = request.max_tokens,
            "Would run inference"
        );

        let elapsed = start.elapsed();

        Ok(GenerateResponse {
            text: format!(
                "[Inference placeholder for model '{}' with prompt: '{}...']",
                request.model,
                request.prompt.chars().take(50).collect::<String>()
            ),
            tokens_generated: 0,
            tokens_per_second: 0.0,
            time_to_first_token_ms: elapsed.as_millis() as u64,
            total_time_ms: elapsed.as_millis() as u64,
            finish_reason: FinishReason::Stop,
            model_id: model_guard.info.id.clone(),
        })
    }

    /// Get memory usage stats.
    pub async fn memory_stats(&self) -> MemoryStats {
        MemoryStats {
            loaded_models: self.cache.model_count().await,
            memory_used_mb: self.cache.memory_usage_mb().await,
            max_memory_mb: self.config.max_memory_mb,
        }
    }
}

/// Model registry for tracking available models.
pub struct ModelRegistry {
    models: std::collections::HashMap<ModelId, ModelInfo>,
}

impl ModelRegistry {
    pub fn new() -> Self {
        Self {
            models: std::collections::HashMap::new(),
        }
    }

    pub fn register(&mut self, info: ModelInfo) {
        self.models.insert(info.id.clone(), info);
    }

    pub fn get(&self, model_id: &str) -> Option<&ModelInfo> {
        self.models.get(model_id)
    }

    pub fn list(&self) -> Vec<ModelInfo> {
        self.models.values().cloned().collect()
    }

    pub fn remove(&mut self, model_id: &str) -> Option<ModelInfo> {
        self.models.remove(model_id)
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Guard that releases model reference when dropped.
struct ModelUseGuard<'a> {
    cache: &'a ModelCache,
    model_id: String,
}

impl<'a> ModelUseGuard<'a> {
    fn new(cache: &'a ModelCache, model_id: &str) -> Self {
        Self {
            cache,
            model_id: model_id.to_string(),
        }
    }
}

impl<'a> Drop for ModelUseGuard<'a> {
    fn drop(&mut self) {
        let cache = self.cache;
        let model_id = self.model_id.clone();

        // We need to spawn because Drop is sync
        tokio::spawn(async move {
            // Can't actually call release here due to lifetime issues
            // This would need a redesign for proper ref counting
            let _ = model_id;
        });
    }
}

/// Inference engine configuration.
#[derive(Debug, Clone)]
pub struct InferenceConfig {
    /// Directory where models are stored
    pub models_dir: PathBuf,
    /// Maximum models to keep loaded
    pub max_loaded_models: usize,
    /// Maximum memory usage in MB
    pub max_memory_mb: u32,
    /// Number of GPU layers to offload (-1 = auto, 0 = CPU only)
    pub gpu_layers: i32,
    /// Context size for inference
    pub context_size: u32,
    /// Batch size for inference
    pub batch_size: u32,
}

impl Default for InferenceConfig {
    fn default() -> Self {
        Self {
            models_dir: crate::bootstrap::base_dir().join("models"),
            max_loaded_models: 3,
            max_memory_mb: 16_000, // 16 GB
            gpu_layers: -1,        // Auto
            context_size: 4096,
            batch_size: 512,
        }
    }
}

/// Request for text generation.
#[derive(Debug, Clone)]
pub struct GenerateRequest {
    /// Model ID to use
    pub model: String,
    /// Input prompt
    pub prompt: String,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Sampling temperature
    pub temperature: f32,
    /// Top-p sampling
    pub top_p: f32,
    /// Stop sequences
    pub stop_sequences: Vec<String>,
}

impl GenerateRequest {
    pub fn new(model: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            prompt: prompt.into(),
            max_tokens: 512,
            temperature: 0.7,
            top_p: 0.9,
            stop_sequences: vec![],
        }
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = temperature;
        self
    }
}

/// Response from text generation.
#[derive(Debug, Clone)]
pub struct GenerateResponse {
    /// Generated text
    pub text: String,
    /// Number of tokens generated
    pub tokens_generated: u32,
    /// Generation speed
    pub tokens_per_second: f64,
    /// Time to first token in ms
    pub time_to_first_token_ms: u64,
    /// Total generation time in ms
    pub total_time_ms: u64,
    /// Why generation stopped
    pub finish_reason: FinishReason,
    /// Model that was used
    pub model_id: ModelId,
}

/// Why generation stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishReason {
    /// Hit a stop sequence
    Stop,
    /// Hit max tokens limit
    Length,
    /// Content filter triggered
    ContentFilter,
}

/// Memory usage statistics.
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub loaded_models: usize,
    pub memory_used_mb: u32,
    pub max_memory_mb: u32,
}

/// Inference errors.
#[derive(Debug, thiserror::Error)]
pub enum InferenceError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Model load failed: {0}")]
    LoadFailed(String),

    #[error("Generation failed: {0}")]
    GenerationFailed(String),

    #[error("Cache error: {0}")]
    CacheError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_inference_engine_creation() {
        let dir = tempdir().unwrap();
        let config = InferenceConfig {
            models_dir: dir.path().to_path_buf(),
            ..Default::default()
        };

        let engine = InferenceEngine::new(config).unwrap();
        assert!(engine.available_models().await.is_empty());
    }

    #[tokio::test]
    async fn test_scan_empty_directory() {
        let dir = tempdir().unwrap();
        let config = InferenceConfig {
            models_dir: dir.path().to_path_buf(),
            ..Default::default()
        };

        let engine = InferenceEngine::new(config).unwrap();
        let count = engine.scan_models().await.unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_model_not_found() {
        let dir = tempdir().unwrap();
        let config = InferenceConfig {
            models_dir: dir.path().to_path_buf(),
            ..Default::default()
        };

        let engine = InferenceEngine::new(config).unwrap();
        let result = engine.load_model("nonexistent").await;
        assert!(matches!(result, Err(InferenceError::ModelNotFound(_))));
    }
}
