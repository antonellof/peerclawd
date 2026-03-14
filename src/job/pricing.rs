//! Resource pricing for job marketplace.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::wallet::{to_micro, from_micro};

/// Types of resources that can be requested/offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceType {
    /// LLM inference
    Inference {
        model: String,
        tokens: u32,
    },
    /// Text embedding generation
    Embedding {
        model: String,
        tokens: u32,
    },
    /// Image generation
    ImageGeneration {
        model: String,
        count: u32,
    },
    /// CPU compute time
    Cpu {
        cores: u16,
        duration_secs: u64,
    },
    /// GPU compute time
    Gpu {
        vram_mb: u32,
        duration_secs: u64,
    },
    /// Storage (read/write)
    Storage {
        operation: StorageOperation,
        bytes: u64,
    },
    /// Web fetch
    WebFetch {
        url_count: u32,
    },
    /// Vector search
    VectorSearch {
        query_count: u32,
    },
    /// WASM tool execution
    WasmTool {
        tool_name: String,
        invocations: u32,
    },
}

/// Storage operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageOperation {
    Read,
    Write,
}

impl fmt::Display for ResourceType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceType::Inference { model, tokens } => {
                write!(f, "Inference: {} ({} tokens)", model, tokens)
            }
            ResourceType::Embedding { model, tokens } => {
                write!(f, "Embedding: {} ({} tokens)", model, tokens)
            }
            ResourceType::ImageGeneration { model, count } => {
                write!(f, "ImageGen: {} ({} images)", model, count)
            }
            ResourceType::Cpu { cores, duration_secs } => {
                write!(f, "CPU: {} cores for {}s", cores, duration_secs)
            }
            ResourceType::Gpu { vram_mb, duration_secs } => {
                write!(f, "GPU: {}MB VRAM for {}s", vram_mb, duration_secs)
            }
            ResourceType::Storage { operation, bytes } => {
                write!(f, "Storage {:?}: {} bytes", operation, bytes)
            }
            ResourceType::WebFetch { url_count } => {
                write!(f, "WebFetch: {} URLs", url_count)
            }
            ResourceType::VectorSearch { query_count } => {
                write!(f, "VectorSearch: {} queries", query_count)
            }
            ResourceType::WasmTool { tool_name, invocations } => {
                write!(f, "WASM {}: {} invocations", tool_name, invocations)
            }
        }
    }
}

/// Base pricing rates for resources (in μPCLAW).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePricing {
    /// Price per 1K tokens for small models (7B-13B)
    pub inference_small_per_1k: u64,
    /// Price per 1K tokens for medium models (30B-70B)
    pub inference_medium_per_1k: u64,
    /// Price per 1K tokens for large models (70B+)
    pub inference_large_per_1k: u64,
    /// Price per 1K tokens for embeddings
    pub embedding_per_1k: u64,
    /// Price per image generated
    pub image_per_image: u64,
    /// Price per CPU core-hour
    pub cpu_per_core_hour: u64,
    /// Price per GPU hour (consumer)
    pub gpu_consumer_per_hour: u64,
    /// Price per GPU hour (datacenter)
    pub gpu_datacenter_per_hour: u64,
    /// Price per MB storage read
    pub storage_read_per_mb: u64,
    /// Price per MB storage write
    pub storage_write_per_mb: u64,
    /// Price per web fetch request
    pub web_fetch_per_request: u64,
    /// Price per vector search query
    pub vector_search_per_query: u64,
    /// Price per WASM tool invocation
    pub wasm_per_invocation: u64,
}

impl Default for ResourcePricing {
    fn default() -> Self {
        Self {
            // From PEERCLAWD-TOKEN-ECONOMY.md indicative rates
            inference_small_per_1k: to_micro(0.5),      // 0.5 PCLAW per 1K tokens
            inference_medium_per_1k: to_micro(2.0),     // 2.0 PCLAW per 1K tokens
            inference_large_per_1k: to_micro(5.0),      // 5.0 PCLAW per 1K tokens
            embedding_per_1k: to_micro(0.2),            // 0.2 PCLAW per 1K tokens
            image_per_image: to_micro(3.0),             // 3.0 PCLAW per image
            cpu_per_core_hour: to_micro(2.0),           // 2.0 PCLAW per core-hour
            gpu_consumer_per_hour: to_micro(15.0),      // 15.0 PCLAW per GPU-hour
            gpu_datacenter_per_hour: to_micro(40.0),    // 40.0 PCLAW per GPU-hour
            storage_read_per_mb: to_micro(0.005),       // 0.005 PCLAW per MB
            storage_write_per_mb: to_micro(0.01),       // 0.01 PCLAW per MB
            web_fetch_per_request: to_micro(0.1),       // 0.1 PCLAW per request
            vector_search_per_query: to_micro(0.05),    // 0.05 PCLAW per query
            wasm_per_invocation: to_micro(0.02),        // 0.02 PCLAW per invocation
        }
    }
}

impl ResourcePricing {
    /// Calculate price for a resource type and quantity.
    pub fn calculate(&self, resource: &ResourceType) -> u64 {
        match resource {
            ResourceType::Inference { model, tokens } => {
                let rate = self.get_inference_rate(model);
                (rate * *tokens as u64) / 1000
            }
            ResourceType::Embedding { tokens, .. } => {
                (self.embedding_per_1k * *tokens as u64) / 1000
            }
            ResourceType::ImageGeneration { count, .. } => {
                self.image_per_image * *count as u64
            }
            ResourceType::Cpu { cores, duration_secs } => {
                // Convert seconds to hours
                let hours = (*duration_secs as f64) / 3600.0;
                (self.cpu_per_core_hour as f64 * *cores as f64 * hours) as u64
            }
            ResourceType::Gpu { duration_secs, .. } => {
                let hours = (*duration_secs as f64) / 3600.0;
                (self.gpu_consumer_per_hour as f64 * hours) as u64
            }
            ResourceType::Storage { operation, bytes } => {
                let mb = (*bytes as f64) / (1024.0 * 1024.0);
                let rate = match operation {
                    StorageOperation::Read => self.storage_read_per_mb,
                    StorageOperation::Write => self.storage_write_per_mb,
                };
                (rate as f64 * mb) as u64
            }
            ResourceType::WebFetch { url_count } => {
                self.web_fetch_per_request * *url_count as u64
            }
            ResourceType::VectorSearch { query_count } => {
                self.vector_search_per_query * *query_count as u64
            }
            ResourceType::WasmTool { invocations, .. } => {
                self.wasm_per_invocation * *invocations as u64
            }
        }
    }

    /// Get inference rate based on model size.
    fn get_inference_rate(&self, model: &str) -> u64 {
        let model_lower = model.to_lowercase();

        // Simple heuristic based on model name
        if model_lower.contains("70b") || model_lower.contains("72b") || model_lower.contains("mixtral") {
            self.inference_large_per_1k
        } else if model_lower.contains("30b") || model_lower.contains("34b") || model_lower.contains("33b") {
            self.inference_medium_per_1k
        } else {
            self.inference_small_per_1k
        }
    }
}

/// Pricing strategy for a peer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PricingStrategy {
    /// Base pricing rates
    pub base_rates: ResourcePricing,
    /// Price multiplier based on current utilization (0.5 - 3.0)
    pub utilization_multiplier: f64,
    /// Target utilization percentage (0.0 - 1.0)
    pub target_utilization: f64,
    /// Estimated latency in milliseconds
    pub estimated_latency_ms: u32,
    /// Reputation score (0.0 - 1.0)
    pub reputation: f64,
}

impl Default for PricingStrategy {
    fn default() -> Self {
        Self {
            base_rates: ResourcePricing::default(),
            utilization_multiplier: 1.0,
            target_utilization: 0.7,
            estimated_latency_ms: 100,
            reputation: 0.5, // Start at neutral
        }
    }
}

impl PricingStrategy {
    /// Calculate price for a resource considering utilization and reputation.
    pub fn calculate_price(&self, resource: &ResourceType, units: u32) -> u64 {
        let base_price = self.base_rates.calculate(resource);
        let adjusted = (base_price as f64 * self.utilization_multiplier) as u64;

        // Apply reputation discount (higher rep = slight discount to win more jobs)
        let rep_factor = 1.0 - (self.reputation * 0.1); // Max 10% discount at rep 1.0
        (adjusted as f64 * rep_factor) as u64
    }

    /// Update utilization multiplier based on current load.
    pub fn update_utilization(&mut self, current_utilization: f64) {
        // If above target, increase prices; if below, decrease
        let diff = current_utilization - self.target_utilization;

        // Adjust multiplier: +/- up to 0.1 per update
        self.utilization_multiplier += diff * 0.2;

        // Clamp to valid range
        self.utilization_multiplier = self.utilization_multiplier.clamp(0.5, 3.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_pricing() {
        let pricing = ResourcePricing::default();

        // Test inference pricing
        let resource = ResourceType::Inference {
            model: "llama-3.2-8b".into(),
            tokens: 1000,
        };
        let price = pricing.calculate(&resource);
        assert_eq!(price, to_micro(0.5)); // 0.5 PCLAW for 1K tokens
    }

    #[test]
    fn test_large_model_pricing() {
        let pricing = ResourcePricing::default();

        let resource = ResourceType::Inference {
            model: "llama-3.3-70b".into(),
            tokens: 2000,
        };
        let price = pricing.calculate(&resource);
        assert_eq!(price, to_micro(10.0)); // 5.0 * 2 = 10.0 PCLAW
    }

    #[test]
    fn test_storage_pricing() {
        let pricing = ResourcePricing::default();

        let resource = ResourceType::Storage {
            operation: StorageOperation::Write,
            bytes: 10 * 1024 * 1024, // 10 MB
        };
        let price = pricing.calculate(&resource);
        assert_eq!(price, to_micro(0.1)); // 0.01 * 10 = 0.1 PCLAW
    }

    #[test]
    fn test_utilization_adjustment() {
        let mut strategy = PricingStrategy::default();

        // High utilization should increase prices
        strategy.update_utilization(0.9);
        assert!(strategy.utilization_multiplier > 1.0);

        // Low utilization should decrease prices
        strategy.update_utilization(0.3);
        // After two updates, should trend back towards 1.0
    }
}
