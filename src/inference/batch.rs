//! Batch Aggregation for Multi-Agent Inference
//!
//! Collects inference requests from multiple sources (agents, peers, API)
//! and processes them together for efficiency. This is especially useful
//! for multi-agent scenarios where many agents submit requests concurrently.
//!
//! # Architecture
//!
//! ```text
//! Agent 1 ──┐
//! Agent 2 ──┼──► BatchQueue ──► BatchExecutor ──► Results
//! Agent 3 ──┤      (50ms window)   (parallel)
//! P2P Peer ─┘
//! ```
//!
//! # Benefits
//! - Reduced model loading overhead (load once, run many)
//! - Better GPU utilization through batched inference
//! - Lower latency for concurrent requests
//! - Efficient P2P bandwidth usage

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, oneshot, RwLock, Mutex};
use uuid::Uuid;

use super::GenerateResponse;

/// Configuration for batch aggregation
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum time to wait for batch to fill (default: 50ms)
    pub batch_window_ms: u64,
    /// Maximum requests per batch (default: 8)
    pub max_batch_size: usize,
    /// Minimum requests to trigger immediate processing (default: 4)
    pub min_batch_size: usize,
    /// Enable adaptive batching based on load
    pub adaptive: bool,
    /// Maximum queue depth before rejecting requests
    pub max_queue_depth: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            batch_window_ms: 50,
            max_batch_size: 8,
            min_batch_size: 4,
            adaptive: true,
            max_queue_depth: 100,
        }
    }
}

/// A single inference request in the batch queue
#[derive(Debug)]
pub struct BatchRequest {
    /// Unique request ID
    pub id: Uuid,
    /// Source identifier (agent ID, peer ID, or "api")
    pub source: String,
    /// Model to use
    pub model: String,
    /// Prompt text
    pub prompt: String,
    /// Maximum tokens to generate
    pub max_tokens: u32,
    /// Temperature for sampling
    pub temperature: f32,
    /// When the request was submitted
    pub submitted_at: Instant,
    /// Channel to send result back
    pub result_tx: oneshot::Sender<Result<BatchResponse, BatchError>>,
}

/// Response from a batched inference request
#[derive(Debug, Clone)]
pub struct BatchResponse {
    /// Request ID
    pub request_id: Uuid,
    /// Generated text
    pub text: String,
    /// Tokens generated
    pub tokens_generated: u32,
    /// Tokens per second
    pub tokens_per_second: f64,
    /// Time spent waiting in queue
    pub queue_time_ms: u64,
    /// Time spent in inference
    pub inference_time_ms: u64,
    /// Batch size this request was part of
    pub batch_size: usize,
}

/// Errors that can occur during batch processing
#[derive(Debug, Clone)]
pub enum BatchError {
    /// Queue is full
    QueueFull,
    /// Request timed out waiting for batch
    Timeout,
    /// Inference failed
    InferenceFailed(String),
    /// Model not found
    ModelNotFound(String),
    /// Batch aggregator is shutting down
    Shutdown,
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchError::QueueFull => write!(f, "Batch queue is full"),
            BatchError::Timeout => write!(f, "Request timed out"),
            BatchError::InferenceFailed(e) => write!(f, "Inference failed: {}", e),
            BatchError::ModelNotFound(m) => write!(f, "Model not found: {}", m),
            BatchError::Shutdown => write!(f, "Batch aggregator is shutting down"),
        }
    }
}

impl std::error::Error for BatchError {}

/// Statistics about batch processing
#[derive(Debug, Clone, Default)]
pub struct BatchStats {
    /// Total requests processed
    pub total_requests: u64,
    /// Total batches executed
    pub total_batches: u64,
    /// Average batch size
    pub avg_batch_size: f32,
    /// Average queue time in ms
    pub avg_queue_time_ms: f32,
    /// Average inference time in ms
    pub avg_inference_time_ms: f32,
    /// Requests currently in queue
    pub queue_depth: usize,
    /// Batches by model
    pub batches_by_model: HashMap<String, u64>,
}

/// Batch Aggregator - collects and processes inference requests in batches
#[allow(dead_code)]
pub struct BatchAggregator {
    config: BatchConfig,
    /// Incoming request queue
    request_tx: mpsc::Sender<BatchRequest>,
    /// Statistics
    stats: Arc<RwLock<BatchStats>>,
    /// Shutdown flag
    shutdown: Arc<RwLock<bool>>,
}

impl BatchAggregator {
    /// Create a new batch aggregator
    pub fn new(config: BatchConfig) -> (Self, BatchProcessor) {
        let (request_tx, request_rx) = mpsc::channel(config.max_queue_depth);
        let stats = Arc::new(RwLock::new(BatchStats::default()));
        let shutdown = Arc::new(RwLock::new(false));

        let aggregator = Self {
            config: config.clone(),
            request_tx,
            stats: stats.clone(),
            shutdown: shutdown.clone(),
        };

        let processor = BatchProcessor {
            config,
            request_rx: Mutex::new(request_rx),
            stats,
            shutdown,
        };

        (aggregator, processor)
    }

    /// Submit an inference request to the batch queue
    pub async fn submit(
        &self,
        source: String,
        model: String,
        prompt: String,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<BatchResponse, BatchError> {
        // Check if shutting down
        if *self.shutdown.read().await {
            return Err(BatchError::Shutdown);
        }

        let (result_tx, result_rx) = oneshot::channel();

        let request = BatchRequest {
            id: Uuid::new_v4(),
            source,
            model,
            prompt,
            max_tokens,
            temperature,
            submitted_at: Instant::now(),
            result_tx,
        };

        // Try to send to queue
        self.request_tx
            .send(request)
            .await
            .map_err(|_| BatchError::QueueFull)?;

        // Update queue depth stat
        {
            let mut stats = self.stats.write().await;
            stats.queue_depth += 1;
        }

        // Wait for result
        result_rx.await.map_err(|_| BatchError::Shutdown)?
    }

    /// Get current statistics
    pub async fn stats(&self) -> BatchStats {
        self.stats.read().await.clone()
    }

    /// Shutdown the aggregator
    pub async fn shutdown(&self) {
        *self.shutdown.write().await = true;
    }
}

/// Batch Processor - runs the batch processing loop
pub struct BatchProcessor {
    config: BatchConfig,
    request_rx: Mutex<mpsc::Receiver<BatchRequest>>,
    stats: Arc<RwLock<BatchStats>>,
    shutdown: Arc<RwLock<bool>>,
}

impl BatchProcessor {
    /// Run the batch processing loop
    /// This should be spawned as a background task
    pub async fn run<F, Fut>(&self, inference_fn: F)
    where
        F: Fn(String, Vec<(Uuid, String, u32, f32)>) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = Vec<(Uuid, Result<GenerateResponse, String>)>> + Send,
    {
        let mut request_rx = self.request_rx.lock().await;

        loop {
            // Check shutdown
            if *self.shutdown.read().await {
                // Drain remaining requests with shutdown error
                while let Ok(req) = request_rx.try_recv() {
                    let _ = req.result_tx.send(Err(BatchError::Shutdown));
                }
                break;
            }

            // Collect batch
            let mut batch: Vec<BatchRequest> = Vec::new();
            let batch_start = Instant::now();
            let batch_window = Duration::from_millis(self.config.batch_window_ms);

            // Wait for first request
            match tokio::time::timeout(Duration::from_millis(100), request_rx.recv()).await {
                Ok(Some(req)) => batch.push(req),
                Ok(None) => break, // Channel closed
                Err(_) => continue, // Timeout, check shutdown and try again
            }

            // Collect more requests within the batch window
            while batch.len() < self.config.max_batch_size {
                let remaining = batch_window.saturating_sub(batch_start.elapsed());
                if remaining.is_zero() {
                    break;
                }

                // Early trigger if we have min_batch_size
                if batch.len() >= self.config.min_batch_size {
                    break;
                }

                match tokio::time::timeout(remaining, request_rx.recv()).await {
                    Ok(Some(req)) => batch.push(req),
                    Ok(None) => break,
                    Err(_) => break, // Window expired
                }
            }

            if batch.is_empty() {
                continue;
            }

            // Update queue depth
            {
                let mut stats = self.stats.write().await;
                stats.queue_depth = stats.queue_depth.saturating_sub(batch.len());
            }

            // Group by model for efficient processing
            let mut by_model: HashMap<String, Vec<BatchRequest>> = HashMap::new();
            for req in batch {
                by_model
                    .entry(req.model.clone())
                    .or_default()
                    .push(req);
            }

            // Process each model group
            for (model, requests) in by_model {
                let batch_size = requests.len();
                let inference_start = Instant::now();

                // Collect request metadata and senders
                let mut request_data: Vec<(Uuid, String, u32, f32, Instant)> = Vec::with_capacity(batch_size);
                let mut result_senders: HashMap<Uuid, oneshot::Sender<Result<BatchResponse, BatchError>>> = HashMap::new();

                for req in requests {
                    request_data.push((req.id, req.prompt, req.max_tokens, req.temperature, req.submitted_at));
                    result_senders.insert(req.id, req.result_tx);
                }

                // Prepare batch input for inference function
                let batch_input: Vec<(Uuid, String, u32, f32)> = request_data
                    .iter()
                    .map(|(id, prompt, max_tokens, temp, _)| (*id, prompt.clone(), *max_tokens, *temp))
                    .collect();

                // Execute batch inference
                let results = inference_fn(model.clone(), batch_input).await;

                let inference_time = inference_start.elapsed();

                // Distribute results to waiting callers
                for (id, result) in results {
                    if let Some(tx) = result_senders.remove(&id) {
                        // Find submit time for this request
                        let submitted_at = request_data.iter()
                            .find(|(req_id, _, _, _, _)| *req_id == id)
                            .map(|(_, _, _, _, t)| *t)
                            .unwrap_or(inference_start);

                        let queue_time = submitted_at.elapsed();

                        let response = match result {
                            Ok(gen) => Ok(BatchResponse {
                                request_id: id,
                                text: gen.text,
                                tokens_generated: gen.tokens_generated,
                                tokens_per_second: gen.tokens_per_second,
                                queue_time_ms: queue_time.as_millis() as u64,
                                inference_time_ms: inference_time.as_millis() as u64,
                                batch_size,
                            }),
                            Err(e) => Err(BatchError::InferenceFailed(e)),
                        };

                        // Send result back to caller (ignore send errors if receiver dropped)
                        let _ = tx.send(response);
                    }
                }

                // Send errors to any requests that didn't get a response
                for (id, tx) in result_senders {
                    let _ = tx.send(Err(BatchError::InferenceFailed(
                        format!("No result returned for request {}", id)
                    )));
                }

                // Update statistics
                {
                    let mut stats = self.stats.write().await;
                    stats.total_batches += 1;
                    stats.total_requests += batch_size as u64;

                    // Update running averages
                    let n = stats.total_batches as f32;
                    stats.avg_batch_size =
                        (stats.avg_batch_size * (n - 1.0) + batch_size as f32) / n;
                    stats.avg_inference_time_ms =
                        (stats.avg_inference_time_ms * (n - 1.0) + inference_time.as_millis() as f32) / n;

                    *stats.batches_by_model.entry(model).or_insert(0) += 1;
                }
            }
        }
    }
}

/// Helper to create a batch-aware inference executor
pub struct BatchInferenceExecutor {
    aggregator: Arc<BatchAggregator>,
}

impl BatchInferenceExecutor {
    pub fn new(aggregator: Arc<BatchAggregator>) -> Self {
        Self { aggregator }
    }

    /// Submit inference request through batch aggregator
    pub async fn infer(
        &self,
        source: &str,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        temperature: f32,
    ) -> Result<BatchResponse, BatchError> {
        self.aggregator
            .submit(
                source.to_string(),
                model.to_string(),
                prompt.to_string(),
                max_tokens,
                temperature,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_batch_aggregator_creation() {
        let config = BatchConfig::default();
        let (aggregator, _processor) = BatchAggregator::new(config);

        let stats = aggregator.stats().await;
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_batches, 0);
    }

    #[tokio::test]
    async fn test_batch_config_defaults() {
        let config = BatchConfig::default();
        assert_eq!(config.batch_window_ms, 50);
        assert_eq!(config.max_batch_size, 8);
        assert_eq!(config.min_batch_size, 4);
        assert!(config.adaptive);
    }
}
