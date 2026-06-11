//! LLM provider trait and implementations.

use async_trait::async_trait;
use futures::FutureExt;
use futures::stream::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use thiserror::Error;

/// Role of a message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A single message in a conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

/// Conversation history for multi-turn LLM interactions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageHistory {
    pub messages: Vec<ChatMessage>,
    pub conversation_id: Option<String>,
}

impl MessageHistory {
    /// Create a new empty message history
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a message to the history
    pub fn push(&mut self, role: ChatRole, content: String) {
        self.messages.push(ChatMessage { role, content });
    }

    /// Returns true if the history has no messages
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Registry for managing named conversations across multiple LLM calls.
/// Enables multi-turn conversations within a single pipeline run.
#[derive(Debug, Clone, Default)]
pub struct ConversationRegistry {
    conversations: std::collections::HashMap<String, MessageHistory>,
}

impl ConversationRegistry {
    /// Create a new empty conversation registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a conversation by ID
    pub fn get_or_create(&mut self, id: &str) -> &mut MessageHistory {
        self.conversations
            .entry(id.to_string())
            .or_insert_with(MessageHistory::new)
    }

    /// Get a conversation by ID without creating it
    pub fn get(&self, id: &str) -> Option<&MessageHistory> {
        self.conversations.get(id)
    }

    /// Insert or replace a conversation
    pub fn insert(&mut self, id: String, history: MessageHistory) {
        self.conversations.insert(id, history);
    }
}

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
}


/// A tool call extracted from LLM response
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
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

    #[error("rate limited")]
    RateLimited,

    #[error("auth failed")]
    AuthFailed,

    #[error("LLM not configured")]
    NotConfigured,
}

/// Provider specification for LLM calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProviderSpec {
    OpenAiCompatible { base_url: String, model: String },
    Builtin(String),
}

/// Trait for LLM providers.
#[async_trait]
pub trait LlmProvider: Send + Sync {
    /// Returns the provider's name.
    fn name(&self) -> &str;

    /// Complete an LLM request.
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;

    /// Stream an LLM request, yielding chunks as they arrive.
    /// For providers that don't natively support streaming, this calls `complete()` and wraps the result in a single-item stream.
    fn stream(
        &self,
        request: LlmRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<LlmChunk, LlmError>> + Send>>;
}

/// OpenAI-compatible provider (e.g., OpenAI, local Ollama, etc.).
pub struct OpenAiCompatibleProvider {
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
    client: Client,
}

impl OpenAiCompatibleProvider {
    /// Create a new OpenAI-compatible provider.
    pub fn new(base_url: String, api_key: String, default_model: String) -> Self {
        Self {
            base_url,
            api_key,
            default_model,
            client: Client::new(),
        }
    }
}


/// Internal structs for deserializing OpenAI API responses.
#[derive(Debug, Deserialize)]
struct OpenAiToolCallFunction {
    name: String,
    arguments: String, // JSON string
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    #[allow(dead_code)]
    id: Option<String>,
    function: OpenAiToolCallFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    model: String,
    usage: Option<OpenAiUsage>,
}


#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    fn name(&self) -> &str {
        "openai-compatible"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // Build the messages array: system first, then history, then new user turn
        let mut messages = vec![
            serde_json::json!({"role": "system", "content": req.system}),
        ];
        if let Some(history) = &req.history {
            for msg in &history.messages {
                let role_str = match msg.role {
                    ChatRole::System => "system",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::Tool => "tool",
                };
                messages.push(serde_json::json!({"role": role_str, "content": msg.content}));
            }
        }
        messages.push(serde_json::json!({"role": "user", "content": req.user}));

        // Build the request body
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": messages,
            "stream": false
        });
        
        // Add optional fields only if present
        if let Some(max_tokens) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temperature) = req.temperature {
            body["temperature"] = serde_json::json!(temperature);
        }

        // Add tool schemas for function calling if provided
        if let Some(tools) = &req.tools {
            let tools_json: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            }).collect();
            if !tools_json.is_empty() {
                body["tools"] = serde_json::json!(tools_json);
                body["tool_choice"] = serde_json::json!("auto");
            }
        }


        // Construct the URL — strip any trailing /v1 from base_url to avoid double-path
        let base = self.base_url.trim_end_matches('/').trim_end_matches("/v1");
        let url = format!("{}/v1/chat/completions", base);

        // Make the HTTP request
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LlmError::NetworkError(e.to_string())
                } else if e.is_connect() {
                    LlmError::NetworkError(e.to_string())
                } else {
                    LlmError::RequestFailed(e.to_string())
                }
            })?;

        // Check status code
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(LlmError::AuthFailed);
        }
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(LlmError::RateLimited);
        }
        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(LlmError::RequestFailed(error_text));
        }

        // Deserialize the response
        let api_response: OpenAiResponse = response
            .json()
            .await
            .map_err(|e| LlmError::InvalidResponse(e.to_string()))?;


        // Extract first choice
        let first_choice = api_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvalidResponse("no choices in response".into()))?;

        // Extract content (may be null/empty when tool_calls are present)
        let content = first_choice.message.content.unwrap_or_default();

        // Parse tool_calls if present
        let tool_calls = first_choice.message.tool_calls.map(|calls| {
            calls.into_iter().filter_map(|tc| {
                // Parse the arguments JSON string
                let arguments = serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                    .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                Some(ToolCall {
                    name: tc.function.name,
                    arguments,
                })
            }).collect::<Vec<_>>()
        }).filter(|v: &Vec<ToolCall>| !v.is_empty());

        // Extract usage if available
        let usage = api_response.usage.map(|u| LlmUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        });

        Ok(LlmResponse {
            content,
            model: api_response.model,
            usage,
            tool_calls,
        })

    }

    fn stream(
        &self,
        request: LlmRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<LlmChunk, LlmError>> + Send>> {
        // Fallback implementation: call complete() and yield the entire response as a single chunk.
        // True HTTP streaming would require stream=true in the API request and SSE parsing.
        use futures::stream::iter;
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let default_model = self.default_model.clone();
        let client = self.client.clone();

        let response_future = async move {
            let provider = OpenAiCompatibleProvider {
                base_url,
                api_key,
                default_model,
                client,
            };
            match provider.complete(request).await {
                Ok(response) => {
                    vec![Ok(LlmChunk {
                        delta: response.content,
                        finish_reason: Some("stop".to_string()),
                    })]
                }
                Err(e) => vec![Err(e)],
            }
        };

        Box::pin(
            response_future
                .map(|vec| iter(vec.into_iter()))
                .flatten_stream(),
        )
    }
}
