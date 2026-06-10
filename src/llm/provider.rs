//! LLM provider trait and implementations.

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Request to an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub system: String,
    pub user: String,
    pub model: String,
    pub max_tokens: Option<u32>,
}

/// Response from an LLM provider.
#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<LlmUsage>,
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

/// Internal struct for deserializing OpenAI API responses.
#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: String,
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
        // Build the request body
        let body = serde_json::json!({
            "model": req.model,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user}
            ],
            "max_tokens": req.max_tokens,
            "stream": false
        });

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

        // Extract content from first choice
        let content = api_response
            .choices
            .first()
            .ok_or_else(|| LlmError::InvalidResponse("no choices in response".into()))?
            .message
            .content
            .clone();

        // Extract usage if available
        let usage = api_response.usage.map(|u| LlmUsage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
        });

        Ok(LlmResponse {
            content,
            model: api_response.model,
            usage,
        })
    }
}
