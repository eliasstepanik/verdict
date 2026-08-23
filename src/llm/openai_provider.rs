//! OpenAI-compatible LLM provider implementation.

// ponytail: this file exceeds the 300-line limit as a documented, user-approved exception.
// Rationale: complete() and stream() are OpenAiCompatibleProvider's LlmProvider trait
// implementation methods, sharing request-building and HTTP error-classification logic
// (now centralized in openai_shared.rs to eliminate duplication — this consolidation
// genuinely fixed a bug where stream() misclassified 429 rate-limit responses that
// complete() correctly identified). Splitting complete()/stream() into separate files
// would re-fragment their shared dependencies and risk reintroducing the exact
// "divergent duplicate path" bug class this session found and fixed 8+ times elsewhere
// (see notes/verdict-audit-cycle1-fix-plan.md, H1/C2/C3/C4 fixes). Approved as an
// escalation-clause exception per AGENTS.md 300-Line File Limit rule (tightly-coupled
// trait-impl methods sharing centralized dependencies). File shrunk from 463 to 382
// lines via the openai_shared.rs extraction; further splitting was assessed as
// disproportionate risk relative to benefit and explicitly declined by the user.

use async_trait::async_trait;
use futures::stream::Stream;
use reqwest::Client;
use std::pin::Pin;
use tracing::{debug, trace};

use super::{ChatRole, LlmChunk, LlmError, LlmProvider, LlmRequest, LlmResponse, LlmUsage, ToolCall};
use crate::llm::openai_types::OpenAiResponse;
use crate::llm::openai_shared::{build_request_body, classify_http_error};

/// OpenAI-compatible provider (e.g., OpenAI, local Ollama, etc.).
pub struct OpenAiCompatibleProvider {
    pub base_url: String,
    pub api_key: String,
    pub default_model: String,
    client: Client,
}

impl OpenAiCompatibleProvider {
    /// Create a new OpenAI-compatible provider with default 120-second timeout.
    pub fn new(base_url: String, api_key: String, default_model: String) -> Self {
        Self {
            base_url,
            api_key,
            default_model,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Create a new OpenAI-compatible provider with custom timeout (in seconds).
    pub fn with_timeout(
        base_url: String,
        api_key: String,
        default_model: String,
        timeout_secs: u64,
    ) -> Self {
        Self {
            base_url,
            api_key,
            default_model,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(timeout_secs))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Build messages array from request and history
    fn build_messages(req: &LlmRequest) -> Vec<serde_json::Value> {
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
                    messages.push(serde_json::json!({
                        "role": role_str,
                        "content": if msg.content.is_empty() { serde_json::Value::Null } else { serde_json::json!(msg.content) },
                        "tool_calls": tool_calls
                    }));
                } else if let Some(tool_call_id) = &msg.tool_call_id {
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
        if !req.user.is_empty() {
            messages.push(serde_json::json!({"role": "user", "content": req.user}));
        }
        messages
    }


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
        let messages = Self::build_messages(&req);
        let body = build_request_body(&req, messages, self.default_model(), false);

        let model = body.get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        let tools_count = body.get("tools")
            .and_then(|t| t.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let tool_choice = body.get("tool_choice")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "none".into());
        debug!(model = %model, tools_count = tools_count, tool_choice = %tool_choice, "LLM request");

        let base = self.base_url.trim_end_matches('/').trim_end_matches("/v1");
        let url = format!("{}/v1/chat/completions", base);

        let mut last_err: Option<LlmError> = None;
        for attempt in 0u32..3 {
            if attempt > 0 {
                let backoff_secs = 2u64.pow(attempt - 1);
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            }

            let response_result = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await;

            let response = match response_result {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(if e.is_timeout() {
                        LlmError::NetworkError(e.to_string())
                    } else if e.is_connect() {
                        LlmError::NetworkError(e.to_string())
                    } else {
                        LlmError::RequestFailed(e.to_string())
                    });
                    continue;
                }
            };

            let status = response.status();
            if !status.is_success() {
                let resp_body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| String::from("(could not read response body)"));

                if let Some(err) = classify_http_error(status, &resp_body, &url) {
                    match err {
                        LlmError::AuthFailed => return Err(err),
                        LlmError::RequestFailed(_) if status.is_client_error() => return Err(err),
                        _ => {
                            last_err = Some(err);
                            continue;
                        }
                    }
                } else {
                    return Err(LlmError::RequestFailed(format!(
                        "HTTP {status} from {url}: {resp_body}"
                    )));
                }
            }

            let raw_body = match response.text().await {
                Ok(b) => b,
                Err(e) => {
                    last_err = Some(LlmError::InvalidResponse(e.to_string()));
                    continue;
                }
            };

            let preview_end = raw_body
                .char_indices()
                .nth(300)
                .map(|(i, _)| i)
                .unwrap_or(raw_body.len());
            let preview = &raw_body[..preview_end];
            let has_tool_calls = raw_body.contains("\"tool_calls\"");
            trace!(status = %status, has_tool_calls = has_tool_calls, body_preview = %preview, "LLM raw response");

            let api_response: OpenAiResponse = match serde_json::from_str(&raw_body) {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(LlmError::InvalidResponse(format!(
                        "{}: body={}",
                        e,
                        &raw_body[..raw_body.len().min(500)]
                    )));
                    continue;
                }
            };

            let first_choice = match api_response.choices.into_iter().next() {
                Some(c) => c,
                None => {
                    last_err = Some(LlmError::InvalidResponse("no choices in response".into()));
                    continue;
                }
            };

            let content = first_choice.message.content.unwrap_or_default();

            let tool_calls = first_choice
                .message
                .tool_calls
                .map(|calls| {
                    calls
                        .into_iter()
                        .filter_map(|tc| {
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

            let usage = api_response.usage.map(|u| LlmUsage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
            });

            return Ok(LlmResponse {
                content,
                model: api_response.model,
                usage,
                tool_calls,
            });
        }

        if let Some(err) = last_err {
            Err(err)
        } else {
            Err(LlmError::RequestFailed(
                "max retries exceeded with no recorded error".into(),
            ))
        }
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

        let messages = Self::build_messages(&request);
        let body = build_request_body(&request, messages, &default_model, true);

        let base = base_url
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .to_string();
        let url = format!("{}/v1/chat/completions", base);

        Box::pin(
            futures::stream::once(async move {
                let resp = http_client
                    .post(&url)
                    .bearer_auth(&api_key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| LlmError::NetworkError(e.to_string()))?;

                let status = resp.status();
                if !status.is_success() {
                    let body_text = resp
                        .text()
                        .await
                        .unwrap_or_else(|_| String::from("(could not read response body)"));
                    if let Some(err) = classify_http_error(status, &body_text, &url) {
                        return Err(err);
                    }
                    return Err(LlmError::RequestFailed(format!(
                        "HTTP {status} from {url}: {body_text}"
                    )));
                }

                let full_text = resp
                    .text()
                    .await
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

                                    if let Some(newline_pos) = response[pos..].find('\n') {
                                        let line_end = pos + newline_pos;
                                        let line = response[pos..line_end]
                                            .trim_end_matches('\r')
                                            .to_string();
                                        pos = line_end + 1;

                                        if line == "data: [DONE]" {
                                            return None;
                                        }

                                        if let Some(data) = line.strip_prefix("data: ") {
                                            if let Ok(json) =
                                                serde_json::from_str::<serde_json::Value>(data)
                                            {
                                                if let Some(choice) = json["choices"].get(0) {
                                                    let delta = choice["delta"]["content"]
                                                        .as_str()
                                                        .unwrap_or("")
                                                        .to_string();
                                                    let finish_reason = choice["finish_reason"]
                                                        .as_str()
                                                        .map(String::from);
                                                    return Some((
                                                        Ok(LlmChunk { delta, finish_reason }),
                                                        (response, pos),
                                                    ));
                                                }
                                            }
                                        }
                                        continue;
                                    } else {
                                        if pos < response.len() {
                                            let remaining = response[pos..].trim();
                                            if let Some(data) = remaining.strip_prefix("data: ") {
                                                if let Ok(json) =
                                                    serde_json::from_str::<serde_json::Value>(data)
                                                {
                                                    if let Some(choice) = json["choices"].get(0) {
                                                        let delta = choice["delta"]["content"]
                                                            .as_str()
                                                            .unwrap_or("")
                                                            .to_string();
                                                        let finish_reason = choice["finish_reason"]
                                                            .as_str()
                                                            .map(String::from);
                                                        let response_len = response.len();
                                                        return Some((
                                                            Ok(LlmChunk { delta, finish_reason }),
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
            }),
        )
    }
}
