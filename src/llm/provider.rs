//! LLM provider trait and implementations.

use async_trait::async_trait;
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
    /// For assistant messages that invoked tools: the raw tool_calls JSON array
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls_json: Option<serde_json::Value>,
    /// For tool result messages: the tool_call_id this result corresponds to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn user(content: String) -> Self {
        Self { role: ChatRole::User, content, tool_calls_json: None, tool_call_id: None }
    }
    pub fn assistant(content: String) -> Self {
        Self { role: ChatRole::Assistant, content, tool_calls_json: None, tool_call_id: None }
    }
    pub fn assistant_with_tool_calls(content: String, tool_calls: serde_json::Value) -> Self {
        Self { role: ChatRole::Assistant, content, tool_calls_json: Some(tool_calls), tool_call_id: None }
    }
    pub fn tool_result(tool_call_id: String, content: String) -> Self {
        Self { role: ChatRole::Tool, content, tool_calls_json: None, tool_call_id: Some(tool_call_id) }
    }
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
    /// Append a message to the history
    pub fn push(&mut self, role: ChatRole, content: String) {
        self.messages.push(ChatMessage { role, content, tool_calls_json: None, tool_call_id: None });
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
    /// Auto-generated titles for conversations (Phase A8)
    titles: std::collections::HashMap<String, String>,
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

    /// Get the auto-generated title for a conversation (Phase A8)
    pub fn get_title(&self, id: &str) -> Option<&str> {
        self.titles.get(id).map(|s| s.as_str())
    }

    /// Set the auto-generated title for a conversation (Phase A8)
    pub fn set_title(&mut self, id: String, title: String) {
        self.titles.insert(id, title);
    }

    /// List all conversations as (id, history) pairs
    pub fn list_conversations(&self) -> Vec<(String, MessageHistory)> {
        self.conversations
            .iter()
            .map(|(id, history)| (id.clone(), history.clone()))
            .collect()
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
    /// Optional tool_choice override. "auto" (default when tools provided), "required" (force tool use), or "none".
    #[serde(default)]
    pub tool_choice: Option<String>,

}

/// A tool call extracted from LLM response
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
    pub id: Option<String>,  // Tool call ID from LLM response
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
#[allow(dead_code)]
struct SseStreamDelta {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SseStreamChoice {
    delta: SseStreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SseStreamChunk {
    choices: Vec<SseStreamChoice>,
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

    fn default_model(&self) -> &str {
        &self.default_model
    }


    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // Build the messages array: system first, then history, then new user turn
        let mut messages = vec![serde_json::json!({"role": "system", "content": req.system})];
        if let Some(history) = &req.history {
            for msg in &history.messages {
                let role_str = match msg.role {
                    ChatRole::System => "system",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::Tool => "tool",
                };
                
                if let Some(tool_calls) = &msg.tool_calls_json {
                    // Assistant message that made tool calls
                    messages.push(serde_json::json!({
                        "role": role_str,
                        "content": if msg.content.is_empty() { serde_json::Value::Null } else { serde_json::json!(msg.content) },
                        "tool_calls": tool_calls
                    }));
                } else if let Some(tool_call_id) = &msg.tool_call_id {
                    // Tool result message
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_call_id,
                        "content": msg.content
                    }));
                } else {
                    messages.push(serde_json::json!({"role": role_str, "content": msg.content}));
                }
            }
        }
        // Only push user message if non-empty — Claude/OpenAI APIs reject empty user messages
        // when the conversation ends with tool result messages (round > 0 in ToolUseLoop).
        if !req.user.is_empty() {
            messages.push(serde_json::json!({"role": "user", "content": req.user}));
        }


        // Use default model if req.model is empty
        let model = if req.model.is_empty() {
            self.default_model().to_string()
        } else {
            req.model.clone()
        };

        // Build the request body
        let mut body = serde_json::json!({
            "model": model,
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
            let tools_json: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.parameters
                        }
                    })
                })
                .collect();
            if !tools_json.is_empty() {
                body["tools"] = serde_json::json!(tools_json);
                let choice = req.tool_choice.as_deref().unwrap_or("auto");
                body["tool_choice"] = serde_json::json!(choice);

            }
        }

        // Debug: log whether tools are included in request
        eprintln!("[llm-req] model={} tools_count={} tool_choice={}", 
            model,
            body.get("tools").and_then(|t| t.as_array()).map(|a| a.len()).unwrap_or(0),
            body.get("tool_choice").map(|v| v.to_string()).unwrap_or_else(|| "none".into()));


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

        // Check status code — always read the body for error context
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| String::from("(could not read response body)"));
            if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
                return Err(LlmError::AuthFailed);
            }
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                return Err(LlmError::RequestFailed(format!(
                    "HTTP 429 from {url}: {body}"
                )));
            }
            return Err(LlmError::RequestFailed(format!(
                "HTTP {status} from {url}: {body}"
            )));
        }


        // Read raw body so we can deserialize
        let raw_body = response.text().await
            .map_err(|e| LlmError::InvalidResponse(e.to_string()))?;

        // Debug: log first 300 chars of raw response to see if tool_calls are present
        let preview_end = raw_body.char_indices().nth(300).map(|(i,_)| i).unwrap_or(raw_body.len());
        eprintln!("[llm-raw] status={} has_tool_calls={} body_preview={}", 
            status,
            raw_body.contains("\"tool_calls\""),
            &raw_body[..preview_end]);

        // Deserialize the response
        let api_response: OpenAiResponse = serde_json::from_str(&raw_body)
            .map_err(|e| LlmError::InvalidResponse(format!("{}: body={}", e, &raw_body[..raw_body.len().min(500)])))?;



        // Extract first choice
        let first_choice = api_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| LlmError::InvalidResponse("no choices in response".into()))?;

        // Extract content (may be null/empty when tool_calls are present)
        let content = first_choice.message.content.unwrap_or_default();

        // Parse tool_calls if present
        let tool_calls = first_choice
            .message
            .tool_calls
            .map(|calls| {
                calls
                    .into_iter()
                    .filter_map(|tc| {
                        // Parse the arguments JSON string
                        let arguments =
                            serde_json::from_str::<serde_json::Value>(&tc.function.arguments)
                                .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
                        Some(ToolCall {
                            name: tc.function.name,
                            arguments,
                            id: tc.id.clone(),

                        })
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v: &Vec<ToolCall>| !v.is_empty());

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
        use futures::StreamExt;

        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let default_model = self.default_model.clone();
        let http_client = self.client.clone();

        let model = if request.model.is_empty() {
            default_model.clone()
        } else {
            request.model.clone()
        };

        let mut messages = vec![serde_json::json!({"role": "system", "content": request.system})];
        if let Some(history) = &request.history {
            for msg in &history.messages {
                let role_str = match msg.role {
                    ChatRole::System => "system",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::Tool => "tool",
                };
                
                if let Some(tool_calls) = &msg.tool_calls_json {
                    // Assistant message that made tool calls
                    messages.push(serde_json::json!({
                        "role": role_str,
                        "content": if msg.content.is_empty() { serde_json::Value::Null } else { serde_json::json!(msg.content) },
                        "tool_calls": tool_calls
                    }));
                } else if let Some(tool_call_id) = &msg.tool_call_id {
                    // Tool result message
                    messages.push(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": tool_call_id,
                        "content": msg.content
                    }));
                } else {
                    messages.push(serde_json::json!({"role": role_str, "content": msg.content}));
                }
            }
        }
        messages.push(serde_json::json!({"role": "user", "content": request.user}));

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": true
        });
        if let Some(mt) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(t) = request.temperature {
            body["temperature"] = serde_json::json!(t);
        }


        let base = base_url.trim_end_matches('/').trim_end_matches("/v1").to_string();
        let url = format!("{}/v1/chat/completions", base);

        // Real SSE streaming with incremental byte processing
        Box::pin(
            futures::stream::once(async move {
                // Make the initial HTTP request
                let resp = http_client
                    .post(&url)
                    .bearer_auth(&api_key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| LlmError::NetworkError(e.to_string()))?;

                let status = resp.status();
                if !status.is_success() {
                    let body_text = resp.text().await
                        .unwrap_or_else(|_| String::from("(could not read response body)"));
                    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
                        return Err(LlmError::AuthFailed);
                    }
                    return Err(LlmError::RequestFailed(format!(
                        "HTTP {status} from {url}: {body_text}"
                    )));
                }


                // Get the full text response (unfortunate but necessary for now without proper SSE lib)
                let full_text = resp.text().await
                    .map_err(|e| LlmError::NetworkError(e.to_string()))?;
                Ok(full_text)
            })
            .flat_map(|text_result| {
                match text_result {
                    Err(e) => futures::stream::once(async move { Err(e) }).boxed(),
                    Ok(full_response) => {
                        use futures::stream::unfold;
                        unfold(
                            (full_response, 0usize),
                            |(response, mut pos): (String, usize)| async move {
                                loop {
                                    if pos >= response.len() {
                                        return None;
                                    }

                                    // Find next newline
                                    if let Some(newline_pos) = response[pos..].find('\n') {
                                        let line_end = pos + newline_pos;
                                        let line = response[pos..line_end].trim_end_matches('\r').to_string();
                                        pos = line_end + 1;

                                        if line == "data: [DONE]" {
                                            return None;
                                        }

                                        if let Some(data) = line.strip_prefix("data: ") {
                                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                                if let Some(choice) = json["choices"].get(0) {
                                                    // Always emit chunks (even when delta.content is missing)
                                                    let delta = choice["delta"]["content"]
                                                        .as_str()
                                                        .unwrap_or("")
                                                        .to_string();
                                                    let finish_reason = choice["finish_reason"].as_str().map(String::from);
                                                    return Some((
                                                        Ok(LlmChunk {
                                                            delta,
                                                            finish_reason,
                                                        }),
                                                        (response, pos),
                                                    ));
                                                }
                                            }
                                        }
                                        continue;
                                    } else {
                                        // Process residual buffer if stream didn't end with newline
                                        if pos < response.len() {
                                            let remaining = response[pos..].trim();
                                            if let Some(data) = remaining.strip_prefix("data: ") {
                                                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                                                    if let Some(choice) = json["choices"].get(0) {
                                                        let delta = choice["delta"]["content"]
                                                            .as_str()
                                                            .unwrap_or("")
                                                            .to_string();
                                                        let finish_reason = choice["finish_reason"].as_str().map(String::from);
                                                        let response_len = response.len();
                                                        return Some((
                                                            Ok(LlmChunk {
                                                                delta,
                                                                finish_reason,
                                                            }),
                                                            (response, response_len),
                                                        ));
                                                    }
                                                }
                                            }
                                        }
                                        return None;
                                    }
                                }
                            },
                        )
                        .boxed()
                    }
                }
            })
        )
    }
}