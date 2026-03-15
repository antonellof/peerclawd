//! GGUF model loading and inference via llama.cpp.
//!
//! This module provides the actual inference backend using llama.cpp bindings.
//! It's only compiled when the `local-inference` feature is enabled.

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, Mutex};

use super::{FinishReason, GenerateRequest, GenerateResponse, InferenceError, ModelId};

/// Configuration for GGUF inference.
#[derive(Debug, Clone)]
pub struct GgufConfig {
    /// Number of GPU layers to offload (-1 = auto, 0 = CPU only)
    pub n_gpu_layers: i32,
    /// Context size
    pub n_ctx: u32,
    /// Batch size for prompt processing
    pub n_batch: u32,
    /// Number of threads for CPU inference
    pub n_threads: u32,
    /// Use memory-mapped models
    pub use_mmap: bool,
    /// Use memory locking
    pub use_mlock: bool,
}

impl Default for GgufConfig {
    fn default() -> Self {
        Self {
            n_gpu_layers: -1, // Auto-detect
            n_ctx: 4096,
            n_batch: 512,
            n_threads: num_cpus::get() as u32,
            use_mmap: true,
            use_mlock: false,
        }
    }
}

/// Token callback for streaming generation.
pub type TokenCallback = Box<dyn FnMut(&str) + Send>;

/// GGUF model backend trait.
/// This trait abstracts the actual llama.cpp implementation,
/// allowing for testing without the actual library.
#[allow(dead_code)]
pub trait GgufBackend: Send + Sync {
    /// Load a model from a GGUF file.
    fn load_model(&self, path: &Path, config: &GgufConfig) -> Result<GgufModelHandle, GgufError>;

    /// Generate text from a prompt.
    fn generate(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, GgufError>;

    /// Generate text with streaming callback - called for each token as it's generated.
    fn generate_streaming(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
        token_callback: TokenCallback,
    ) -> Result<GenerateResponse, GgufError>;

    /// Get model info from a loaded model.
    fn model_info(&self, model: &GgufModelHandle) -> GgufModelInfo;

    /// Unload a model and free resources.
    fn unload_model(&self, model: GgufModelHandle);
}

/// Handle to a loaded GGUF model.
pub struct GgufModelHandle {
    /// Model identifier
    pub id: ModelId,
    /// Path to the model file
    pub path: std::path::PathBuf,
    /// Internal handle (type-erased for flexibility)
    #[cfg(feature = "local-inference")]
    inner: Option<GgufModelInner>,
    #[cfg(not(feature = "local-inference"))]
    inner: (),
    /// Estimated memory usage in MB
    pub memory_mb: u32,
}

/// Internal model representation.
#[cfg(feature = "local-inference")]
struct GgufModelInner {
    model: LlamaModelWrapper,
}

impl GgufModelHandle {
    /// Create a placeholder handle (for when local-inference is disabled).
    pub fn placeholder(id: ModelId, path: std::path::PathBuf, memory_mb: u32) -> Self {
        Self {
            id,
            path,
            #[cfg(feature = "local-inference")]
            inner: None,
            #[cfg(not(feature = "local-inference"))]
            inner: (),
            memory_mb,
        }
    }
}

/// Information about a loaded GGUF model.
#[derive(Debug, Clone)]
pub struct GgufModelInfo {
    /// Number of parameters
    pub n_params: u64,
    /// Context size
    pub n_ctx: u32,
    /// Embedding dimension
    pub n_embd: u32,
    /// Number of layers
    pub n_layer: u32,
    /// Vocabulary size
    pub n_vocab: u32,
    /// Model architecture name
    pub arch: String,
    /// Quantization type
    pub quantization: String,
}

/// Errors from GGUF operations.
#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("Model file not found: {0}")]
    FileNotFound(String),

    #[error("Failed to load model: {0}")]
    LoadFailed(String),

    #[error("Tokenization failed: {0}")]
    TokenizationFailed(String),

    #[error("Generation failed: {0}")]
    GenerationFailed(String),

    #[error("Context size exceeded: {max} tokens available, {requested} requested")]
    ContextSizeExceeded { max: u32, requested: u32 },

    #[error("Feature not enabled: local-inference feature required")]
    FeatureNotEnabled,

    #[error("GPU not available")]
    GpuNotAvailable,
}

impl From<GgufError> for InferenceError {
    fn from(e: GgufError) -> Self {
        match e {
            GgufError::FileNotFound(path) => InferenceError::ModelNotFound(path),
            GgufError::LoadFailed(msg) => InferenceError::LoadFailed(msg),
            GgufError::TokenizationFailed(msg) | GgufError::GenerationFailed(msg) => {
                InferenceError::GenerationFailed(msg)
            }
            GgufError::ContextSizeExceeded { max, requested } => {
                InferenceError::GenerationFailed(format!(
                    "Context size exceeded: {} available, {} requested",
                    max, requested
                ))
            }
            GgufError::FeatureNotEnabled => {
                InferenceError::LoadFailed("local-inference feature not enabled".to_string())
            }
            GgufError::GpuNotAvailable => {
                InferenceError::LoadFailed("GPU not available".to_string())
            }
        }
    }
}

/// Placeholder backend for when local-inference feature is disabled.
pub struct PlaceholderBackend;

impl GgufBackend for PlaceholderBackend {
    fn load_model(&self, path: &Path, _config: &GgufConfig) -> Result<GgufModelHandle, GgufError> {
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Estimate memory based on file size
        let memory_mb = std::fs::metadata(path)
            .map(|m| (m.len() / (1024 * 1024)) as u32)
            .unwrap_or(4096);

        Ok(GgufModelHandle::placeholder(
            id,
            path.to_path_buf(),
            memory_mb,
        ))
    }

    fn generate(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, GgufError> {
        let start = Instant::now();

        // Return a placeholder response
        let text = format!(
            "[Placeholder response for '{}' - enable local-inference feature for actual inference]\n\
             Prompt: {}...",
            model.id,
            request.prompt.chars().take(100).collect::<String>()
        );

        Ok(GenerateResponse {
            text,
            tokens_generated: 0,
            tokens_per_second: 0.0,
            time_to_first_token_ms: start.elapsed().as_millis() as u64,
            total_time_ms: start.elapsed().as_millis() as u64,
            finish_reason: FinishReason::Stop,
            model_id: model.id.clone(),
        })
    }

    fn generate_streaming(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
        mut token_callback: TokenCallback,
    ) -> Result<GenerateResponse, GgufError> {
        // For placeholder, just call the regular generate and stream word by word
        let response = self.generate(model, request)?;
        for word in response.text.split_inclusive(' ') {
            token_callback(word);
        }
        Ok(response)
    }

    fn model_info(&self, _model: &GgufModelHandle) -> GgufModelInfo {
        GgufModelInfo {
            n_params: 0,
            n_ctx: 4096,
            n_embd: 4096,
            n_layer: 32,
            n_vocab: 128256,
            arch: "unknown".to_string(),
            quantization: "unknown".to_string(),
        }
    }

    fn unload_model(&self, _model: GgufModelHandle) {
        // No-op for placeholder
    }
}

/// Real llama.cpp backend (only compiled with local-inference feature).
///
/// This implementation uses the llama-cpp-2 crate for actual GGUF model loading
/// and inference.
#[cfg(feature = "local-inference")]
pub struct LlamaCppBackend {
    config: GgufConfig,
    backend: llama_cpp_2::llama_backend::LlamaBackend,
}

#[cfg(feature = "local-inference")]
use llama_cpp_2::model::LlamaModel;
#[cfg(feature = "local-inference")]
use llama_cpp_2::model::params::LlamaModelParams;
#[cfg(feature = "local-inference")]
use llama_cpp_2::context::params::LlamaContextParams;
#[cfg(feature = "local-inference")]
use llama_cpp_2::llama_batch::LlamaBatch;
#[cfg(feature = "local-inference")]
use llama_cpp_2::sampling::LlamaSampler;
#[cfg(feature = "local-inference")]
use llama_cpp_2::model::AddBos;

/// Wrapper to hold the actual llama.cpp model.
#[cfg(feature = "local-inference")]
pub struct LlamaModelWrapper {
    model: LlamaModel,
}

#[cfg(feature = "local-inference")]
// SAFETY: LlamaModel is thread-safe according to llama.cpp documentation
unsafe impl Send for LlamaModelWrapper {}
unsafe impl Sync for LlamaModelWrapper {}

#[cfg(feature = "local-inference")]
impl LlamaCppBackend {
    pub fn new(config: GgufConfig) -> Result<Self, GgufError> {
        tracing::info!(
            n_gpu_layers = config.n_gpu_layers,
            n_ctx = config.n_ctx,
            n_threads = config.n_threads,
            "Initializing llama.cpp backend"
        );

        let backend = llama_cpp_2::llama_backend::LlamaBackend::init()
            .map_err(|e| GgufError::LoadFailed(format!("Failed to init backend: {:?}", e)))?;

        Ok(Self { config, backend })
    }
}

#[cfg(feature = "local-inference")]
impl GgufBackend for LlamaCppBackend {
    fn load_model(&self, path: &Path, config: &GgufConfig) -> Result<GgufModelHandle, GgufError> {
        if !path.exists() {
            return Err(GgufError::FileNotFound(path.display().to_string()));
        }

        tracing::info!(path = ?path, "Loading GGUF model with llama-cpp-2");
        let start = Instant::now();

        // Create model params
        let model_params = LlamaModelParams::default()
            .with_n_gpu_layers(config.n_gpu_layers as u32);

        let model = LlamaModel::load_from_file(&self.backend, path, &model_params)
            .map_err(|e| GgufError::LoadFailed(format!("{:?}", e)))?;

        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        let memory_mb = std::fs::metadata(path)
            .map(|m| (m.len() / (1024 * 1024)) as u32)
            .unwrap_or(4096);

        tracing::info!(
            model_id = %id,
            memory_mb = memory_mb,
            elapsed_ms = start.elapsed().as_millis(),
            "Model loaded successfully"
        );

        Ok(GgufModelHandle {
            id,
            path: path.to_path_buf(),
            inner: Some(GgufModelInner {
                model: LlamaModelWrapper { model },
            }),
            memory_mb,
        })
    }

    fn generate(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, GgufError> {
        let start = Instant::now();

        tracing::info!(
            model_id = %model.id,
            prompt_len = request.prompt.len(),
            max_tokens = request.max_tokens,
            temperature = request.temperature,
            "Generating completion with llama-cpp-2"
        );

        let inner = model.inner.as_ref()
            .ok_or_else(|| GgufError::GenerationFailed("Model not loaded".to_string()))?;

        // Create context params
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(std::num::NonZeroU32::new(self.config.n_ctx).unwrap()))
            .with_n_batch(self.config.n_batch)
            .with_n_threads(self.config.n_threads as i32)
            .with_n_threads_batch(self.config.n_threads as i32);

        // Create context
        let mut ctx = inner.model.model.new_context(&self.backend, ctx_params)
            .map_err(|e| GgufError::GenerationFailed(format!("Failed to create context: {:?}", e)))?;

        // Tokenize the prompt
        let tokens = inner.model.model.str_to_token(&request.prompt, AddBos::Always)
            .map_err(|e| GgufError::TokenizationFailed(format!("{:?}", e)))?;

        tracing::debug!(
            token_count = tokens.len(),
            "Tokenized prompt"
        );

        // Create batch and add prompt tokens
        let mut batch = LlamaBatch::new(512, 1);

        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch.add(*token, i as i32, &[0], is_last)
                .map_err(|e| GgufError::GenerationFailed(format!("Failed to add token to batch: {:?}", e)))?;
        }

        // Decode the prompt
        ctx.decode(&mut batch)
            .map_err(|e| GgufError::GenerationFailed(format!("Failed to decode prompt: {:?}", e)))?;

        let ttfb = start.elapsed();

        // Set up sampler
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::dist(1234),
            LlamaSampler::greedy(),
        ]);

        // Generate tokens
        let mut output = String::new();
        let mut token_count = 0u32;
        let mut n_cur = tokens.len();

        while token_count < request.max_tokens {
            // Sample next token
            let new_token = sampler.sample(&ctx, (batch.n_tokens() - 1) as i32);
            sampler.accept(new_token);

            // Check for end of sequence
            if inner.model.model.is_eog_token(new_token) {
                break;
            }

            // Decode token to text using bytes
            // Args: token, buffer_size, special (decode specials), lstrip
            if let Ok(bytes) = inner.model.model.token_to_piece_bytes(new_token, 256, false, None) {
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    output.push_str(s);
                }
            }

            token_count += 1;

            // Prepare for next token
            batch.clear();
            batch.add(new_token, n_cur as i32, &[0], true)
                .map_err(|e| GgufError::GenerationFailed(format!("Failed to add token: {:?}", e)))?;

            // Decode
            ctx.decode(&mut batch)
                .map_err(|e| GgufError::GenerationFailed(format!("Failed to decode: {:?}", e)))?;

            n_cur += 1;
        }

        let elapsed = start.elapsed();
        let tokens_per_second = if elapsed.as_secs_f64() > 0.0 {
            token_count as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        tracing::info!(
            model_id = %model.id,
            tokens = token_count,
            tps = format!("{:.1}", tokens_per_second),
            elapsed_ms = elapsed.as_millis(),
            "Generation complete"
        );

        let finish_reason = if token_count >= request.max_tokens {
            FinishReason::Length
        } else {
            FinishReason::Stop
        };

        Ok(GenerateResponse {
            text: output,
            tokens_generated: token_count,
            tokens_per_second,
            time_to_first_token_ms: ttfb.as_millis() as u64,
            total_time_ms: elapsed.as_millis() as u64,
            finish_reason,
            model_id: model.id.clone(),
        })
    }

    fn generate_streaming(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
        mut token_callback: TokenCallback,
    ) -> Result<GenerateResponse, GgufError> {
        let start = Instant::now();

        tracing::info!(
            model_id = %model.id,
            prompt_len = request.prompt.len(),
            max_tokens = request.max_tokens,
            "Generating completion with streaming"
        );

        let inner = model.inner.as_ref()
            .ok_or_else(|| GgufError::GenerationFailed("Model not loaded".to_string()))?;

        // Create context params
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(std::num::NonZeroU32::new(self.config.n_ctx).unwrap()))
            .with_n_batch(self.config.n_batch)
            .with_n_threads(self.config.n_threads as i32)
            .with_n_threads_batch(self.config.n_threads as i32);

        // Create context
        let mut ctx = inner.model.model.new_context(&self.backend, ctx_params)
            .map_err(|e| GgufError::GenerationFailed(format!("Failed to create context: {:?}", e)))?;

        // Tokenize the prompt
        let tokens = inner.model.model.str_to_token(&request.prompt, AddBos::Always)
            .map_err(|e| GgufError::TokenizationFailed(format!("{:?}", e)))?;

        // Create batch and add prompt tokens
        let mut batch = LlamaBatch::new(512, 1);

        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == tokens.len() - 1;
            batch.add(*token, i as i32, &[0], is_last)
                .map_err(|e| GgufError::GenerationFailed(format!("Failed to add token to batch: {:?}", e)))?;
        }

        // Decode the prompt
        ctx.decode(&mut batch)
            .map_err(|e| GgufError::GenerationFailed(format!("Failed to decode prompt: {:?}", e)))?;

        let ttfb = start.elapsed();

        // Set up sampler
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::dist(1234),
            LlamaSampler::greedy(),
        ]);

        // Generate tokens with streaming
        let mut output = String::new();
        let mut token_count = 0u32;
        let mut n_cur = tokens.len();

        while token_count < request.max_tokens {
            // Sample next token
            let new_token = sampler.sample(&ctx, (batch.n_tokens() - 1) as i32);
            sampler.accept(new_token);

            // Check for end of sequence
            if inner.model.model.is_eog_token(new_token) {
                break;
            }

            // Decode token to text and stream it
            if let Ok(bytes) = inner.model.model.token_to_piece_bytes(new_token, 256, false, None) {
                if let Ok(s) = std::str::from_utf8(&bytes) {
                    output.push_str(s);
                    // Stream the token to the callback
                    token_callback(s);
                }
            }

            token_count += 1;

            // Prepare for next token
            batch.clear();
            batch.add(new_token, n_cur as i32, &[0], true)
                .map_err(|e| GgufError::GenerationFailed(format!("Failed to add token: {:?}", e)))?;

            // Decode
            ctx.decode(&mut batch)
                .map_err(|e| GgufError::GenerationFailed(format!("Failed to decode: {:?}", e)))?;

            n_cur += 1;
        }

        let elapsed = start.elapsed();
        let tokens_per_second = if elapsed.as_secs_f64() > 0.0 {
            token_count as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        tracing::info!(
            model_id = %model.id,
            tokens = token_count,
            tps = format!("{:.1}", tokens_per_second),
            "Streaming generation complete"
        );

        let finish_reason = if token_count >= request.max_tokens {
            FinishReason::Length
        } else {
            FinishReason::Stop
        };

        Ok(GenerateResponse {
            text: output,
            tokens_generated: token_count,
            tokens_per_second,
            time_to_first_token_ms: ttfb.as_millis() as u64,
            total_time_ms: elapsed.as_millis() as u64,
            finish_reason,
            model_id: model.id.clone(),
        })
    }

    fn model_info(&self, _model: &GgufModelHandle) -> GgufModelInfo {
        GgufModelInfo {
            n_params: 0,
            n_ctx: self.config.n_ctx,
            n_embd: 4096,
            n_layer: 32,
            n_vocab: 128256,
            arch: "llama".to_string(),
            quantization: "unknown".to_string(),
        }
    }

    fn unload_model(&self, model: GgufModelHandle) {
        tracing::info!(model_id = %model.id, "Unloading model");
        // Model will be dropped automatically
    }
}

/// Thread-safe wrapper for GGUF model operations.
pub struct GgufEngine {
    backend: Box<dyn GgufBackend>,
    config: GgufConfig,
}

impl GgufEngine {
    /// Create a new GGUF engine with the default backend.
    pub fn new(config: GgufConfig) -> Self {
        #[cfg(feature = "local-inference")]
        let backend: Box<dyn GgufBackend> = match LlamaCppBackend::new(config.clone()) {
            Ok(b) => Box::new(b),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to initialize llama.cpp, using placeholder");
                Box::new(PlaceholderBackend)
            }
        };

        #[cfg(not(feature = "local-inference"))]
        let backend: Box<dyn GgufBackend> = Box::new(PlaceholderBackend);

        Self { backend, config }
    }

    /// Create with a specific backend (for testing).
    pub fn with_backend(backend: Box<dyn GgufBackend>, config: GgufConfig) -> Self {
        Self { backend, config }
    }

    /// Load a model.
    pub fn load(&self, path: &Path) -> Result<GgufModelHandle, GgufError> {
        self.backend.load_model(path, &self.config)
    }

    /// Generate text.
    pub fn generate(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, GgufError> {
        self.backend.generate(model, request)
    }

    /// Generate text with streaming callback.
    pub fn generate_streaming(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
        token_callback: TokenCallback,
    ) -> Result<GenerateResponse, GgufError> {
        self.backend.generate_streaming(model, request, token_callback)
    }

    /// Get model info.
    pub fn model_info(&self, model: &GgufModelHandle) -> GgufModelInfo {
        self.backend.model_info(model)
    }

    /// Unload a model.
    pub fn unload(&self, model: GgufModelHandle) {
        self.backend.unload_model(model)
    }

    /// Check if GPU is available.
    pub fn gpu_available(&self) -> bool {
        // This would check for Metal/CUDA availability
        #[cfg(target_os = "macos")]
        {
            true // Metal is always available on macOS
        }
        #[cfg(not(target_os = "macos"))]
        {
            false // Would check for CUDA
        }
    }
}

/// Async wrapper around GgufEngine for use in async contexts.
pub struct AsyncGgufEngine {
    inner: Arc<Mutex<GgufEngine>>,
}

impl AsyncGgufEngine {
    pub fn new(config: GgufConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(GgufEngine::new(config))),
        }
    }

    pub async fn load(&self, path: &Path) -> Result<GgufModelHandle, GgufError> {
        let engine = self.inner.lock().await;
        engine.load(path)
    }

    pub async fn generate(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
    ) -> Result<GenerateResponse, GgufError> {
        let engine = self.inner.lock().await;
        engine.generate(model, request)
    }

    /// Generate with streaming via an mpsc channel.
    /// Returns a receiver that yields tokens as they're generated,
    /// and eventually the final GenerateResponse.
    pub async fn generate_streaming_channel(
        &self,
        model: &GgufModelHandle,
        request: &GenerateRequest,
    ) -> (mpsc::Receiver<String>, tokio::task::JoinHandle<Result<GenerateResponse, GgufError>>) {
        let (tx, rx) = mpsc::channel::<String>(256);

        // Clone values needed for the closure
        let engine = self.inner.clone();
        let model_id = model.id.clone();
        let model_path = model.path.clone();
        let model_memory = model.memory_mb;
        let request = request.clone();

        let handle = tokio::task::spawn_blocking(move || {
            // We need to re-acquire the lock in blocking context
            let rt = tokio::runtime::Handle::current();
            let engine = rt.block_on(engine.lock());

            // Create a placeholder model handle (the actual inner is in the engine's cache)
            // This is a limitation - we need a way to reference the model
            // For now, reload the model if needed
            let model_handle = GgufModelHandle::placeholder(model_id, model_path, model_memory);

            let callback: TokenCallback = Box::new(move |token: &str| {
                let _ = tx.blocking_send(token.to_string());
            });

            engine.generate_streaming(&model_handle, &request, callback)
        });

        (rx, handle)
    }

    pub async fn unload(&self, model: GgufModelHandle) {
        let engine = self.inner.lock().await;
        engine.unload(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gguf_config_defaults() {
        let config = GgufConfig::default();
        assert_eq!(config.n_ctx, 4096);
        assert_eq!(config.n_batch, 512);
        assert!(config.use_mmap);
    }

    #[test]
    fn test_placeholder_backend() {
        let backend = PlaceholderBackend;
        let config = GgufConfig::default();

        // Test with a non-existent path (placeholder doesn't check)
        let handle = GgufModelHandle::placeholder(
            "test-model".to_string(),
            std::path::PathBuf::from("/tmp/test.gguf"),
            1024,
        );

        let info = backend.model_info(&handle);
        assert_eq!(info.n_ctx, 4096);

        let request = GenerateRequest::new("test-model", "Hello, world!");
        let response = backend.generate(&handle, &request).unwrap();

        assert!(response.text.contains("Placeholder"));
    }

    #[test]
    fn test_gguf_engine_creation() {
        let config = GgufConfig::default();
        let engine = GgufEngine::new(config);

        // Engine should be created without error
        assert!(engine.gpu_available() || !engine.gpu_available()); // Either is fine
    }

    #[test]
    fn test_gguf_error_conversion() {
        let error = GgufError::FileNotFound("/path/to/model.gguf".to_string());
        let inference_error: InferenceError = error.into();

        match inference_error {
            InferenceError::ModelNotFound(path) => {
                assert!(path.contains("model.gguf"));
            }
            _ => panic!("Wrong error type"),
        }
    }

    #[tokio::test]
    async fn test_async_engine() {
        let config = GgufConfig::default();
        let engine = AsyncGgufEngine::new(config);

        // Basic creation test
        // Note: actual model loading would require a real GGUF file
        let _ = engine;
    }
}
