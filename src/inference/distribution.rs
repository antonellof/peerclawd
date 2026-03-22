//! P2P model distribution for sharing models across the network.
//!
//! Models are split into chunks, hashed with BLAKE3, and can be
//! requested from peers who advertise them via the DHT.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use super::model::ModelId;

/// Size of each chunk for model transfer (256 KB).
pub const CHUNK_SIZE: usize = 256 * 1024;

/// Model distributor handles P2P model sharing.
pub struct ModelDistributor {
    /// Directory where models are stored
    models_dir: PathBuf,
    /// Models we have locally and can share
    available_models: RwLock<HashMap<ModelId, ModelMetadata>>,
    /// Models we're currently downloading
    pending_downloads: RwLock<HashMap<ModelId, DownloadState>>,
    /// Known model providers from DHT
    providers: RwLock<HashMap<ModelId, Vec<String>>>,
}

/// Metadata about a model available for distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    /// Model identifier
    pub id: ModelId,
    /// Total size in bytes
    pub size_bytes: u64,
    /// Number of chunks
    pub chunk_count: u32,
    /// BLAKE3 hash of the complete file
    pub file_hash: String,
    /// Hashes of individual chunks
    pub chunk_hashes: Vec<String>,
    /// Model filename
    pub filename: String,
}

impl ModelMetadata {
    /// Create metadata from a model file.
    pub fn from_file(id: ModelId, path: &std::path::Path) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        let metadata = file.metadata()?;
        let size_bytes = metadata.len();
        let chunk_count = size_bytes.div_ceil(CHUNK_SIZE as u64) as u32;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}.gguf", id));

        // Calculate hashes (in a real implementation, this would be done in chunks)
        let contents = std::fs::read(path)?;
        let file_hash = blake3::hash(&contents).to_hex().to_string();

        // Calculate chunk hashes
        let chunk_hashes: Vec<String> = contents
            .chunks(CHUNK_SIZE)
            .map(|chunk| blake3::hash(chunk).to_hex().to_string())
            .collect();

        Ok(Self {
            id,
            size_bytes,
            chunk_count,
            file_hash,
            chunk_hashes,
            filename,
        })
    }

    /// Verify a chunk matches its expected hash.
    pub fn verify_chunk(&self, index: u32, data: &[u8]) -> bool {
        if let Some(expected_hash) = self.chunk_hashes.get(index as usize) {
            let actual_hash = blake3::hash(data).to_hex().to_string();
            &actual_hash == expected_hash
        } else {
            false
        }
    }
}

/// State of an ongoing model download.
#[derive(Debug, Clone)]
pub struct DownloadState {
    /// Model metadata
    pub metadata: ModelMetadata,
    /// Chunks we have received
    pub received_chunks: Vec<bool>,
    /// Temporary file path
    pub temp_path: PathBuf,
    /// Peer we're downloading from
    pub source_peer: String,
    /// Download started at
    pub started_at: std::time::Instant,
    /// Bytes downloaded so far
    pub bytes_received: u64,
}

impl DownloadState {
    /// Create a new download state.
    pub fn new(metadata: ModelMetadata, temp_dir: &std::path::Path, peer: String) -> Self {
        let temp_path = temp_dir.join(format!("{}.partial", metadata.id));
        let received = vec![false; metadata.chunk_count as usize];

        Self {
            metadata,
            received_chunks: received,
            temp_path,
            source_peer: peer,
            started_at: std::time::Instant::now(),
            bytes_received: 0,
        }
    }

    /// Mark a chunk as received.
    pub fn mark_received(&mut self, index: u32, size: usize) {
        if let Some(received) = self.received_chunks.get_mut(index as usize) {
            if !*received {
                *received = true;
                self.bytes_received += size as u64;
            }
        }
    }

    /// Check if download is complete.
    pub fn is_complete(&self) -> bool {
        self.received_chunks.iter().all(|r| *r)
    }

    /// Get download progress (0.0 - 1.0).
    pub fn progress(&self) -> f64 {
        let received = self.received_chunks.iter().filter(|r| **r).count();
        received as f64 / self.metadata.chunk_count as f64
    }

    /// Get next missing chunk index.
    pub fn next_missing_chunk(&self) -> Option<u32> {
        self.received_chunks
            .iter()
            .position(|r| !*r)
            .map(|i| i as u32)
    }

    /// Get download speed in bytes per second.
    pub fn download_speed(&self) -> f64 {
        let elapsed = self.started_at.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.bytes_received as f64 / elapsed
        } else {
            0.0
        }
    }
}

/// Message types for model distribution protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelDistributionMessage {
    /// Announce that we have a model available
    Announce(ModelAnnouncement),
    /// Request model metadata
    MetadataRequest { model_id: String },
    /// Response with model metadata
    MetadataResponse { metadata: ModelMetadata },
    /// Request a chunk of a model
    ChunkRequest {
        model_id: String,
        chunk_index: u32,
    },
    /// Response with chunk data
    ChunkResponse {
        model_id: String,
        chunk_index: u32,
        data: Vec<u8>,
        hash: String,
    },
}

/// Announcement that a peer has a model available.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelAnnouncement {
    /// Model ID
    pub model_id: String,
    /// Model size in bytes
    pub size_bytes: u64,
    /// Model file hash
    pub file_hash: String,
    /// Peer ID of the provider
    pub provider_peer_id: String,
    /// Provider's advertised download speed
    pub bandwidth_mbps: Option<u32>,
}

impl ModelDistributor {
    /// Create a new model distributor.
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            available_models: RwLock::new(HashMap::new()),
            pending_downloads: RwLock::new(HashMap::new()),
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Scan models directory and build list of available models.
    pub async fn scan_available_models(&self) -> std::io::Result<()> {
        let mut available = self.available_models.write().await;
        available.clear();

        if !self.models_dir.exists() {
            return Ok(());
        }

        let entries = std::fs::read_dir(&self.models_dir)?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "gguf") {
                // Extract model ID from filename (e.g., "llama-3.2-3b.Q4_K_M.gguf")
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let model_id: ModelId = stem.to_string();

                    match ModelMetadata::from_file(model_id.clone(), &path) {
                        Ok(metadata) => {
                            tracing::info!(
                                model_id = %model_id,
                                size_mb = metadata.size_bytes / (1024 * 1024),
                                chunks = metadata.chunk_count,
                                "Indexed model for distribution"
                            );
                            available.insert(model_id, metadata);
                        }
                        Err(e) => {
                            tracing::warn!(
                                path = ?path,
                                error = %e,
                                "Failed to index model"
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Get list of models we can share.
    pub async fn available_models(&self) -> Vec<ModelId> {
        self.available_models.read().await.keys().cloned().collect()
    }

    /// Get metadata for a model.
    pub async fn get_metadata(&self, model_id: &ModelId) -> Option<ModelMetadata> {
        self.available_models.read().await.get(model_id).cloned()
    }

    /// Check if we have a model locally.
    pub async fn has_model(&self, model_id: &ModelId) -> bool {
        self.available_models.read().await.contains_key(model_id)
    }

    /// Read a chunk of a model file.
    pub async fn read_chunk(&self, model_id: &ModelId, chunk_index: u32) -> Option<Vec<u8>> {
        let metadata = self.available_models.read().await.get(model_id)?.clone();

        let path = self.models_dir.join(&metadata.filename);
        let file = std::fs::File::open(&path).ok()?;

        let offset = chunk_index as u64 * CHUNK_SIZE as u64;
        let remaining = metadata.size_bytes.saturating_sub(offset);
        let chunk_size = std::cmp::min(remaining, CHUNK_SIZE as u64) as usize;

        if chunk_size == 0 {
            return None;
        }

        use std::io::{Read, Seek, SeekFrom};
        let mut file = file;
        file.seek(SeekFrom::Start(offset)).ok()?;

        let mut buffer = vec![0u8; chunk_size];
        file.read_exact(&mut buffer).ok()?;

        Some(buffer)
    }

    /// Register a model provider from the network.
    pub async fn register_provider(&self, model_id: ModelId, peer_id: String) {
        let mut providers = self.providers.write().await;
        providers
            .entry(model_id)
            .or_insert_with(Vec::new)
            .push(peer_id);
    }

    /// Get known providers for a model.
    pub async fn get_providers(&self, model_id: &ModelId) -> Vec<String> {
        self.providers
            .read()
            .await
            .get(model_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Start downloading a model from a peer.
    pub async fn start_download(
        &self,
        metadata: ModelMetadata,
        source_peer: String,
    ) -> Result<(), DistributionError> {
        let model_id = metadata.id.clone();

        // Check we don't already have it
        if self.has_model(&model_id).await {
            return Err(DistributionError::ModelExists(model_id.clone()));
        }

        // Check we're not already downloading it
        if self.pending_downloads.read().await.contains_key(&model_id) {
            return Err(DistributionError::AlreadyDownloading(model_id.clone()));
        }

        // Create temp directory if needed
        let temp_dir = self.models_dir.join("partial");
        std::fs::create_dir_all(&temp_dir)
            .map_err(|e| DistributionError::IoError(e.to_string()))?;

        // Create download state
        let state = DownloadState::new(metadata, &temp_dir, source_peer);

        // Create partial file
        let file = std::fs::File::create(&state.temp_path)
            .map_err(|e| DistributionError::IoError(e.to_string()))?;
        file.set_len(state.metadata.size_bytes)
            .map_err(|e| DistributionError::IoError(e.to_string()))?;

        self.pending_downloads
            .write()
            .await
            .insert(model_id, state);

        Ok(())
    }

    /// Write a received chunk to the partial file.
    pub async fn write_chunk(
        &self,
        model_id: &ModelId,
        chunk_index: u32,
        data: &[u8],
    ) -> Result<bool, DistributionError> {
        let mut downloads = self.pending_downloads.write().await;
        let state = downloads
            .get_mut(model_id)
            .ok_or_else(|| DistributionError::NotDownloading(model_id.clone()))?;

        // Verify chunk hash
        if !state.metadata.verify_chunk(chunk_index, data) {
            return Err(DistributionError::ChunkVerificationFailed {
                model_id: model_id.clone(),
                chunk_index,
            });
        }

        // Write to file
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&state.temp_path)
            .map_err(|e| DistributionError::IoError(e.to_string()))?;

        let offset = chunk_index as u64 * CHUNK_SIZE as u64;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| DistributionError::IoError(e.to_string()))?;
        file.write_all(data)
            .map_err(|e| DistributionError::IoError(e.to_string()))?;

        // Mark as received
        state.mark_received(chunk_index, data.len());

        tracing::debug!(
            model_id = %model_id,
            chunk = chunk_index,
            progress = format!("{:.1}%", state.progress() * 100.0),
            "Received chunk"
        );

        Ok(state.is_complete())
    }

    /// Complete a download by moving the file to final location.
    pub async fn complete_download(&self, model_id: &ModelId) -> Result<PathBuf, DistributionError> {
        let state = self
            .pending_downloads
            .write()
            .await
            .remove(model_id)
            .ok_or_else(|| DistributionError::NotDownloading(model_id.clone()))?;

        if !state.is_complete() {
            return Err(DistributionError::DownloadIncomplete(model_id.clone()));
        }

        // Verify complete file hash
        let file_contents = std::fs::read(&state.temp_path)
            .map_err(|e| DistributionError::IoError(e.to_string()))?;
        let file_hash = blake3::hash(&file_contents).to_hex().to_string();

        if file_hash != state.metadata.file_hash {
            std::fs::remove_file(&state.temp_path).ok();
            return Err(DistributionError::FileVerificationFailed {
                model_id: model_id.clone(),
                expected: state.metadata.file_hash.clone(),
                actual: file_hash,
            });
        }

        // Move to final location
        let final_path = self.models_dir.join(&state.metadata.filename);
        std::fs::rename(&state.temp_path, &final_path)
            .map_err(|e| DistributionError::IoError(e.to_string()))?;

        // Add to available models
        self.available_models
            .write()
            .await
            .insert(model_id.clone(), state.metadata);

        tracing::info!(
            model_id = %model_id,
            path = ?final_path,
            "Model download complete"
        );

        Ok(final_path)
    }

    /// Get download progress for a model.
    pub async fn download_progress(&self, model_id: &ModelId) -> Option<DownloadProgress> {
        let downloads = self.pending_downloads.read().await;
        downloads.get(model_id).map(|state| DownloadProgress {
            model_id: model_id.clone(),
            progress: state.progress(),
            bytes_received: state.bytes_received,
            total_bytes: state.metadata.size_bytes,
            speed_bps: state.download_speed(),
            source_peer: state.source_peer.clone(),
        })
    }

    /// Cancel a download.
    pub async fn cancel_download(&self, model_id: &ModelId) -> bool {
        if let Some(state) = self.pending_downloads.write().await.remove(model_id) {
            std::fs::remove_file(&state.temp_path).ok();
            tracing::info!(model_id = %model_id, "Download cancelled");
            true
        } else {
            false
        }
    }

    /// Generate announcements for all available models.
    pub async fn create_announcements(&self, local_peer_id: &str) -> Vec<ModelAnnouncement> {
        self.available_models
            .read()
            .await
            .values()
            .map(|meta| ModelAnnouncement {
                model_id: meta.id.clone(),
                size_bytes: meta.size_bytes,
                file_hash: meta.file_hash.clone(),
                provider_peer_id: local_peer_id.to_string(),
                bandwidth_mbps: None, // TODO: Measure actual bandwidth
            })
            .collect()
    }
}

/// Download progress information.
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub model_id: ModelId,
    pub progress: f64,
    pub bytes_received: u64,
    pub total_bytes: u64,
    pub speed_bps: f64,
    pub source_peer: String,
}

impl DownloadProgress {
    /// Estimated time remaining in seconds.
    pub fn eta_secs(&self) -> Option<f64> {
        if self.speed_bps > 0.0 {
            let remaining = self.total_bytes - self.bytes_received;
            Some(remaining as f64 / self.speed_bps)
        } else {
            None
        }
    }
}

/// Errors from model distribution.
#[derive(Debug, thiserror::Error)]
pub enum DistributionError {
    #[error("Model already exists: {0}")]
    ModelExists(String),

    #[error("Already downloading: {0}")]
    AlreadyDownloading(String),

    #[error("Not downloading: {0}")]
    NotDownloading(String),

    #[error("Download incomplete: {0}")]
    DownloadIncomplete(String),

    #[error("Chunk verification failed for {model_id} chunk {chunk_index}")]
    ChunkVerificationFailed { model_id: String, chunk_index: u32 },

    #[error("File verification failed for {model_id}: expected {expected}, got {actual}")]
    FileVerificationFailed {
        model_id: String,
        expected: String,
        actual: String,
    },

    #[error("IO error: {0}")]
    IoError(String),

    #[error("No providers for model: {0}")]
    NoProviders(String),

    #[error("Network error: {0}")]
    NetworkError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_chunk_hashing() {
        let data = b"Hello, World!";
        let hash = blake3::hash(data).to_hex().to_string();
        assert!(!hash.is_empty());
    }

    #[tokio::test]
    async fn test_distributor_creation() {
        let dir = tempdir().unwrap();
        let distributor = ModelDistributor::new(dir.path().to_path_buf());

        assert!(distributor.available_models().await.is_empty());
    }

    #[tokio::test]
    async fn test_provider_registration() {
        let dir = tempdir().unwrap();
        let distributor = ModelDistributor::new(dir.path().to_path_buf());

        let model_id: ModelId = "test-model".to_string();
        distributor
            .register_provider(model_id.clone(), "peer123".to_string())
            .await;

        let providers = distributor.get_providers(&model_id).await;
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0], "peer123");
    }

    #[test]
    fn test_download_state_progress() {
        let metadata = ModelMetadata {
            id: "test".to_string(),
            size_bytes: CHUNK_SIZE as u64 * 4,
            chunk_count: 4,
            file_hash: "test".to_string(),
            chunk_hashes: vec![
                "hash0".to_string(),
                "hash1".to_string(),
                "hash2".to_string(),
                "hash3".to_string(),
            ],
            filename: "test.gguf".to_string(),
        };

        let mut state = DownloadState::new(
            metadata,
            std::path::Path::new("/tmp"),
            "peer1".to_string(),
        );

        assert_eq!(state.progress(), 0.0);
        assert!(!state.is_complete());

        state.mark_received(0, CHUNK_SIZE);
        state.mark_received(1, CHUNK_SIZE);
        assert_eq!(state.progress(), 0.5);

        state.mark_received(2, CHUNK_SIZE);
        state.mark_received(3, CHUNK_SIZE);
        assert!(state.is_complete());
        assert_eq!(state.progress(), 1.0);
    }

    #[test]
    fn test_model_announcement_serialization() {
        let announcement = ModelAnnouncement {
            model_id: "llama-3.2-3b".to_string(),
            size_bytes: 1024 * 1024 * 1024,
            file_hash: "abc123".to_string(),
            provider_peer_id: "12D3KooW...".to_string(),
            bandwidth_mbps: Some(100),
        };

        let bytes = rmp_serde::to_vec(&announcement).unwrap();
        let decoded: ModelAnnouncement = rmp_serde::from_slice(&bytes).unwrap();

        assert_eq!(decoded.model_id, "llama-3.2-3b");
        assert_eq!(decoded.bandwidth_mbps, Some(100));
    }
}
