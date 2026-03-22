//! Model types and metadata.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Unique model identifier (typically the filename without extension).
pub type ModelId = String;

/// Information about a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Unique identifier
    pub id: ModelId,
    /// Human-readable name
    pub name: String,
    /// Path to the model file
    pub path: PathBuf,
    /// File size in bytes
    pub size_bytes: u64,
    /// Model architecture
    pub architecture: ModelArchitecture,
    /// Quantization type
    pub quantization: Quantization,
    /// Number of parameters in billions
    pub parameters_billions: f32,
    /// Maximum context length
    pub context_length: u32,
    /// BLAKE3 hash of the model file
    pub hash: Option<String>,
}

impl ModelInfo {
    /// Create from a GGUF file path.
    pub fn from_path(path: PathBuf) -> std::io::Result<Self> {
        let metadata = std::fs::metadata(&path)?;
        let filename = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Try to parse model info from filename
        let (architecture, parameters, quantization) = parse_model_filename(&filename);

        Ok(Self {
            id: filename.clone(),
            name: filename,
            path,
            size_bytes: metadata.len(),
            architecture,
            quantization,
            parameters_billions: parameters,
            context_length: 4096, // Default, should be read from GGUF metadata
            hash: None,
        })
    }

    /// Estimate RAM requirements in MB.
    pub fn estimate_ram_mb(&self) -> u32 {
        // Rough estimation based on quantization and parameters
        let base_size = match self.quantization {
            Quantization::F32 => self.parameters_billions * 4000.0,
            Quantization::F16 => self.parameters_billions * 2000.0,
            Quantization::Q8_0 => self.parameters_billions * 1000.0,
            Quantization::Q6_K => self.parameters_billions * 750.0,
            Quantization::Q5_K_M => self.parameters_billions * 625.0,
            Quantization::Q4_K_M => self.parameters_billions * 500.0,
            Quantization::Q4_0 => self.parameters_billions * 500.0,
            Quantization::Q3_K_M => self.parameters_billions * 375.0,
            Quantization::Q2_K => self.parameters_billions * 250.0,
            Quantization::Unknown => self.parameters_billions * 500.0, // Assume Q4
        };

        // Add overhead for KV cache (estimate 1GB for 4K context)
        (base_size + 1000.0) as u32
    }

    /// Estimate VRAM requirements in MB (for GPU offload).
    pub fn estimate_vram_mb(&self, gpu_layers: i32) -> u32 {
        if gpu_layers == 0 {
            return 0;
        }

        // Estimate based on model layers (typical: 32-80 layers)
        let total_layers = match self.parameters_billions {
            p if p <= 3.0 => 26,
            p if p <= 8.0 => 32,
            p if p <= 14.0 => 40,
            p if p <= 35.0 => 60,
            p if p <= 70.0 => 80,
            _ => 96,
        };

        let layers_to_offload = if gpu_layers < 0 {
            total_layers // -1 means all layers
        } else {
            gpu_layers.min(total_layers)
        };

        let layer_size_mb = self.estimate_ram_mb() / total_layers as u32;
        layers_to_offload as u32 * layer_size_mb
    }
}

/// Model architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelArchitecture {
    #[default]
    Llama,
    Mistral,
    Phi,
    Qwen,
    Gemma,
    StableLM,
    Falcon,
    MPT,
    Unknown,
}

impl ModelArchitecture {
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("llama") {
            Self::Llama
        } else if lower.contains("mistral") {
            Self::Mistral
        } else if lower.contains("phi") {
            Self::Phi
        } else if lower.contains("qwen") {
            Self::Qwen
        } else if lower.contains("gemma") {
            Self::Gemma
        } else if lower.contains("stablelm") {
            Self::StableLM
        } else if lower.contains("falcon") {
            Self::Falcon
        } else if lower.contains("mpt") {
            Self::MPT
        } else {
            Self::Unknown
        }
    }
}

/// Quantization type.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Quantization {
    F32,
    F16,
    Q8_0,
    Q6_K,
    Q5_K_M,
    #[default]
    Q4_K_M,
    Q4_0,
    Q3_K_M,
    Q2_K,
    Unknown,
}

impl Quantization {
    pub fn from_name(name: &str) -> Self {
        let lower = name.to_lowercase();
        if lower.contains("f32") {
            Self::F32
        } else if lower.contains("f16") {
            Self::F16
        } else if lower.contains("q8_0") {
            Self::Q8_0
        } else if lower.contains("q6_k") {
            Self::Q6_K
        } else if lower.contains("q5_k_m") || lower.contains("q5_k-m") {
            Self::Q5_K_M
        } else if lower.contains("q4_k_m") || lower.contains("q4_k-m") {
            Self::Q4_K_M
        } else if lower.contains("q4_0") {
            Self::Q4_0
        } else if lower.contains("q3_k_m") || lower.contains("q3_k-m") {
            Self::Q3_K_M
        } else if lower.contains("q2_k") {
            Self::Q2_K
        } else {
            Self::Unknown
        }
    }

    /// Bits per weight.
    pub fn bits_per_weight(&self) -> f32 {
        match self {
            Self::F32 => 32.0,
            Self::F16 => 16.0,
            Self::Q8_0 => 8.0,
            Self::Q6_K => 6.5,
            Self::Q5_K_M => 5.5,
            Self::Q4_K_M => 4.5,
            Self::Q4_0 => 4.0,
            Self::Q3_K_M => 3.5,
            Self::Q2_K => 2.5,
            Self::Unknown => 4.5,
        }
    }
}

/// Parse model info from filename.
/// Example: "llama-3.2-8b-instruct-q4_k_m.gguf" -> (Llama, 8.0, Q4_K_M)
fn parse_model_filename(filename: &str) -> (ModelArchitecture, f32, Quantization) {
    let lower = filename.to_lowercase();

    let architecture = ModelArchitecture::from_name(&lower);
    let quantization = Quantization::from_name(&lower);

    // Parse parameter count
    let params = parse_parameters(&lower);

    (architecture, params, quantization)
}

/// Parse parameter count from model name.
fn parse_parameters(name: &str) -> f32 {
    // Look for patterns like "7b", "8b", "70b", "3.2b"
    for part in name.split(&['-', '_', ' ', '.'][..]) {
        if part.ends_with('b') {
            if let Ok(num) = part.trim_end_matches('b').parse::<f32>() {
                return num;
            }
        }
    }
    7.0 // Default
}

/// Resource requirements for running a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequirements {
    /// Minimum RAM needed in MB
    pub min_ram_mb: u32,
    /// Recommended RAM in MB
    pub recommended_ram_mb: u32,
    /// Minimum VRAM for GPU offload in MB
    pub min_vram_mb: Option<u32>,
    /// Number of GPU layers that can be offloaded
    pub gpu_layers: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_model_filename() {
        let (arch, params, quant) = parse_model_filename("llama-3.2-8b-instruct-q4_k_m.gguf");
        assert_eq!(arch, ModelArchitecture::Llama);
        assert!((params - 8.0).abs() < 0.1);
        assert_eq!(quant, Quantization::Q4_K_M);
    }

    #[test]
    fn test_quantization_from_name() {
        assert_eq!(Quantization::from_name("model-q4_k_m.gguf"), Quantization::Q4_K_M);
        assert_eq!(Quantization::from_name("model-q8_0.gguf"), Quantization::Q8_0);
        assert_eq!(Quantization::from_name("model-f16.gguf"), Quantization::F16);
    }

    #[test]
    fn test_architecture_from_name() {
        assert_eq!(ModelArchitecture::from_name("llama-7b"), ModelArchitecture::Llama);
        assert_eq!(ModelArchitecture::from_name("mistral-7b"), ModelArchitecture::Mistral);
        assert_eq!(ModelArchitecture::from_name("phi-2"), ModelArchitecture::Phi);
    }
}
