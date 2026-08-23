//! Internal structs for deserializing OpenAI-compatible API responses.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OpenAiToolCallFunction {
    pub name: String,
    pub arguments: String, // JSON string
}

#[derive(Debug, Deserialize)]
pub struct OpenAiToolCall {
    #[allow(dead_code)]
    pub id: Option<String>,
    pub function: OpenAiToolCallFunction,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SseStreamDelta {
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SseStreamChoice {
    pub delta: SseStreamDelta,
    #[serde(default)]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct SseStreamChunk {
    pub choices: Vec<SseStreamChoice>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiChoice {
    pub message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
}

#[derive(Debug, Deserialize)]
pub struct OpenAiResponse {
    pub choices: Vec<OpenAiChoice>,
    pub model: String,
    pub usage: Option<OpenAiUsage>,
}
