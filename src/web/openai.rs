//! OpenAI-compatible API endpoints.
//!
//! Provides compatibility with OpenAI SDK clients via:
//! - POST /v1/chat/completions
//! - GET /v1/models
//! - POST /v1/embeddings (stub)

use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json, Response,
    },
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::{InferenceRequest, WebState};
use crate::bootstrap;

// ============================================================================
// Request Types
// ============================================================================

/// OpenAI-compatible chat completion request.
#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OpenAiMessage>,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub stream: Option<bool>,
    #[serde(default)]
    pub stop: Option<serde_json::Value>,
    #[serde(default)]
    pub top_p: Option<f32>,
    #[serde(default)]
    pub frequency_penalty: Option<f32>,
    #[serde(default)]
    pub presence_penalty: Option<f32>,
    #[serde(default)]
    pub user: Option<String>,
}

/// OpenAI message format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

// ============================================================================
// Response Types (Non-streaming)
// ============================================================================

/// OpenAI-compatible chat completion response.
#[derive(Debug, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: OpenAiMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

// ============================================================================
// Streaming Response Types
// ============================================================================

/// OpenAI-compatible streaming chunk.
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: u32,
    pub delta: Delta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

// ============================================================================
// Models Endpoint Types
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub object: &'static str,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: &'static str,
    pub created: u64,
    pub owned_by: String,
}

// ============================================================================
// Embeddings Endpoint Types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct EmbeddingsRequest {
    pub model: String,
    pub input: EmbeddingInput,
    #[serde(default)]
    pub encoding_format: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum EmbeddingInput {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Serialize)]
pub struct EmbeddingsResponse {
    pub object: &'static str,
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingData {
    pub object: &'static str,
    pub index: u32,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Serialize)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

// ============================================================================
// Error Response
// ============================================================================

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Debug, Serialize)]
pub struct ErrorDetail {
    pub message: String,
    #[serde(rename = "type")]
    pub error_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

fn error_response(message: &str, error_type: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: ErrorDetail {
                message: message.to_string(),
                error_type: error_type.to_string(),
                param: None,
                code: None,
            },
        }),
    )
}

// ============================================================================
// Utility Functions
// ============================================================================

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn generate_completion_id() -> String {
    format!("chatcmpl-{}", &uuid::Uuid::new_v4().to_string().replace("-", "")[..24])
}

/// Convert OpenAI messages to a prompt string.
fn messages_to_prompt(messages: &[OpenAiMessage]) -> String {
    let mut prompt = String::new();

    for msg in messages {
        let content = msg.content.as_deref().unwrap_or("");
        match msg.role.as_str() {
            "system" => {
                prompt.push_str(&format!("System: {}\n\n", content));
            }
            "user" => {
                prompt.push_str(&format!("User: {}\n", content));
            }
            "assistant" => {
                prompt.push_str(&format!("Assistant: {}\n\n", content));
            }
            _ => {
                prompt.push_str(&format!("{}: {}\n", msg.role, content));
            }
        }
    }

    prompt.push_str("Assistant:");
    prompt
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /v1/chat/completions
pub async fn chat_completions(
    State(state): State<Arc<WebState>>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let stream = req.stream.unwrap_or(false);

    if stream {
        handle_streaming(state, req).await
    } else {
        handle_non_streaming(state, req).await
    }
}

async fn handle_non_streaming(
    state: Arc<WebState>,
    req: ChatCompletionRequest,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let inference_tx = state.inference_tx.as_ref().ok_or_else(|| {
        error_response(
            "Inference not available. Start node with inference support.",
            "service_unavailable",
        )
    })?;

    let prompt = messages_to_prompt(&req.messages);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    let inference_req = InferenceRequest {
        prompt: prompt.clone(),
        model: req.model.clone(),
        max_tokens: req.max_tokens.unwrap_or(500),
        temperature: req.temperature.unwrap_or(0.7),
        response_tx,
    };

    inference_tx.send(inference_req).await.map_err(|_| {
        error_response("Failed to send inference request", "internal_error")
    })?;

    let response = tokio::time::timeout(Duration::from_secs(120), response_rx)
        .await
        .map_err(|_| error_response("Inference timeout", "timeout"))?
        .map_err(|_| error_response("Inference cancelled", "cancelled"))?;

    let completion_id = generate_completion_id();
    let created = unix_timestamp();

    // Estimate token counts (rough: 4 chars per token)
    let prompt_tokens = (prompt.len() / 4) as u32;
    let completion_tokens = response.tokens_generated;

    let response_json = ChatCompletionResponse {
        id: completion_id,
        object: "chat.completion",
        created,
        model: req.model,
        choices: vec![Choice {
            index: 0,
            message: OpenAiMessage {
                role: "assistant".to_string(),
                content: Some(response.text),
                name: None,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
        },
        system_fingerprint: None,
    };

    Ok(Json(response_json).into_response())
}

async fn handle_streaming(
    state: Arc<WebState>,
    req: ChatCompletionRequest,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let inference_tx = state.inference_tx.as_ref().ok_or_else(|| {
        error_response(
            "Inference not available. Start node with inference support.",
            "service_unavailable",
        )
    })?;

    let prompt = messages_to_prompt(&req.messages);
    let (response_tx, response_rx) = tokio::sync::oneshot::channel();

    let inference_req = InferenceRequest {
        prompt,
        model: req.model.clone(),
        max_tokens: req.max_tokens.unwrap_or(500),
        temperature: req.temperature.unwrap_or(0.7),
        response_tx,
    };

    inference_tx.send(inference_req).await.map_err(|_| {
        error_response("Failed to send inference request", "internal_error")
    })?;

    // Wait for the full response first (we simulate streaming)
    let response = tokio::time::timeout(Duration::from_secs(120), response_rx)
        .await
        .map_err(|_| error_response("Inference timeout", "timeout"))?
        .map_err(|_| error_response("Inference cancelled", "cancelled"))?;

    let completion_id = generate_completion_id();
    let created = unix_timestamp();
    let model = req.model.clone();
    let text = response.text;

    // Create SSE stream
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(64);

    tokio::spawn(async move {
        // Send initial chunk with role
        let initial_chunk = ChatCompletionChunk {
            id: completion_id.clone(),
            object: "chat.completion.chunk",
            created,
            model: model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta {
                    role: Some("assistant".to_string()),
                    content: None,
                },
                finish_reason: None,
            }],
        };

        let _ = tx
            .send(Ok(Event::default().data(serde_json::to_string(&initial_chunk).unwrap())))
            .await;

        // Stream content in word-boundary chunks
        let words: Vec<&str> = text.split_inclusive(' ').collect();
        for (i, word) in words.iter().enumerate() {
            let content_chunk = ChatCompletionChunk {
                id: completion_id.clone(),
                object: "chat.completion.chunk",
                created,
                model: model.clone(),
                choices: vec![ChunkChoice {
                    index: 0,
                    delta: Delta {
                        role: None,
                        content: Some(word.to_string()),
                    },
                    finish_reason: None,
                }],
            };

            let _ = tx
                .send(Ok(Event::default().data(serde_json::to_string(&content_chunk).unwrap())))
                .await;

            // Small delay for streaming effect (20ms per word chunk)
            if i < words.len() - 1 {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }

        // Send finish chunk
        let finish_chunk = ChatCompletionChunk {
            id: completion_id.clone(),
            object: "chat.completion.chunk",
            created,
            model: model.clone(),
            choices: vec![ChunkChoice {
                index: 0,
                delta: Delta {
                    role: None,
                    content: None,
                },
                finish_reason: Some("stop".to_string()),
            }],
        };

        let _ = tx
            .send(Ok(Event::default().data(serde_json::to_string(&finish_chunk).unwrap())))
            .await;

        // Send [DONE] sentinel
        let _ = tx.send(Ok(Event::default().data("[DONE]"))).await;
    });

    let stream = ReceiverStream::new(rx);
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}

/// GET /v1/models
pub async fn list_models(State(_state): State<Arc<WebState>>) -> Json<ModelsResponse> {
    let models_dir = bootstrap::base_dir().join("models");
    let created = unix_timestamp();

    let models: Vec<ModelInfo> = std::fs::read_dir(&models_dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .is_some_and(|ext| ext == "gguf")
                })
                .map(|e| {
                    let file_name = e.file_name().to_string_lossy().to_string();
                    let model_id = file_name
                        .strip_suffix(".gguf")
                        .unwrap_or(&file_name)
                        .to_string();
                    ModelInfo {
                        id: model_id,
                        object: "model",
                        created,
                        owned_by: "peerclaw".to_string(),
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    Json(ModelsResponse {
        object: "list",
        data: models,
    })
}

/// POST /v1/embeddings (stub - not implemented)
pub async fn embeddings(
    Json(_req): Json<EmbeddingsRequest>,
) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(ErrorResponse {
            error: ErrorDetail {
                message: "Embeddings are not yet implemented. This endpoint is reserved for future use.".to_string(),
                error_type: "not_implemented".to_string(),
                param: None,
                code: Some("embeddings_not_available".to_string()),
            },
        }),
    )
}
