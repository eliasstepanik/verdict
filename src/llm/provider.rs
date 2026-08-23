//! LLM provider trait and implementations.

use async_trait::async_trait;
use futures::stream::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

pub use crate::llm::messages::{ChatMessage, ChatRole, ConversationRegistry, MessageHistory};
pub use crate::llm::openai_provider::OpenAiCompatibleProvider;
pub use crate::llm::openai_types::{
    OpenAiChoice, OpenAiMessage, OpenAiResponse, OpenAiToolCall, OpenAiToolCallFunction,
    OpenAiUsage, SseStreamChunk, SseStreamChoice, SseStreamDelta,
};

/// A streaming chunk from an LLM provider
#[derive(Debug, Clone)]
pub struct LlmChunk {
    /// The incremental text delta in this chunk
    pub delta: String,
    /// Reason the stream finished, if this is the final chunk
    pub finish_reason: Option<String>,
}

/// Tool schema sent to the LLM for function calling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Request to an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub system: String,
    pub user: String,
    pub model: String,
    pub max_tokens: Option<u32>,
    /// Optional conversation history for multi-turn interactions.
    /// When present, messages are prepended before the current user turn.
    pub history: Option<MessageHistory>,
    /// Optional temperature for sampling (0.0 to 2.0).
    /// Higher values = more creative, lower values = more deterministic.
    pub temperature: Option<f32>,
    /// Optional tool schemas for function/tool calling.
    /// When present, the LLM may respond with tool_calls instead of plain text.
    #[serde(default)]
    pub tools: Option<Vec<ToolSchema>>,
    /// Optional tool_choice override. "auto" (default when tools provided), "required" (force tool use), or "none".
    #[serde(default)]
    pub tool_choice: Option<String>,
}

/// A tool call extracted from LLM response
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    pub id: Option<String>, // Tool call ID from LLM response
}

/// Response from an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<LlmUsage>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

/// Token usage statistics from LLM response.
#[derive(Debug, Clone)]
pub struct LlmUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

/// Errors that can occur when calling an LLM provider.
#[derive(Debug, Error)]
pub enum LlmError {
    #[error("request failed: {0}")]
    RequestFailed(String),

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("rate limited (HTTP 429) — check your quota or try again later")]
    RateLimited,

    #[error("authentication failed — check your API key (OPENAI_API_KEY)")]
    AuthFailed,

    #[error("LLM not configured — set OPENAI_API_KEY or add api_key to ~/.config/verdict-app/config.toml")]
    NotConfigured,
}

/// Trait for LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Returns the provider's name.
    fn name(&self) -> &str;

    /// Complete an LLM request.

    /// Returns the default model name for this provider.
    fn default_model(&self) -> &str;

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;

    /// Stream an LLM request, yielding chunks as they arrive.
    /// For providers that don't natively support streaming, this calls `complete()` and wraps the result in a single-item stream.
    fn stream(
        &self,
        request: LlmRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<LlmChunk, LlmError>> + Send>>;
}
