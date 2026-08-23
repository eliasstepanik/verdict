//! Shared request/response logic for OpenAI-compatible providers.

use reqwest::StatusCode;
use serde_json::json;

use super::{LlmError, LlmRequest};

/// Build the JSON request body for OpenAI API calls.
///
/// Handles model selection, message marshaling, tool definitions, and optional parameters
/// (max_tokens, temperature) consistently between streaming and non-streaming calls.
pub fn build_request_body(
    req: &LlmRequest,
    messages: Vec<serde_json::Value>,
    default_model: &str,
    streaming: bool,
) -> serde_json::Value {
    let model = if req.model.is_empty() {
        default_model.to_string()
    } else {
        req.model.clone()
    };

    let mut body = json!({
        "model": model,
        "messages": messages,
        "stream": streaming
    });

    if let Some(max_tokens) = req.max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }
    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }

    if let Some(tools) = &req.tools {
        let tools_json: Vec<serde_json::Value> = tools
            .iter()
            .map(|t| {
                json!({
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
            body["tools"] = json!(tools_json);
            let choice_value = match req.tool_choice.as_deref() {
                Some("required") => json!({"type": "any"}),
                Some("none") => json!({"type": "none"}),
                _ => json!({"type": "auto"}),
            };
            body["tool_choice"] = choice_value;
        }
    }

    body
}

/// Classify HTTP error responses from the LLM API.
///
/// Distinguishes between authentication failures, rate limiting, client errors, and server errors.
/// This ensures consistent error handling across both streaming and non-streaming calls.
///
/// # Arguments
/// * `status` - The HTTP status code from the response
/// * `body` - The response body text (for error messages)
/// * `url` - The request URL (for error context)
///
/// # Returns
/// Some(LlmError) if the status represents a classified error, None if the status was unexpected.
pub fn classify_http_error(status: StatusCode, body: &str, url: &str) -> Option<LlmError> {
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Some(LlmError::AuthFailed);
    }
    if status == StatusCode::TOO_MANY_REQUESTS {
        return Some(LlmError::RateLimited);
    }
    if status.is_client_error() {
        return Some(LlmError::RequestFailed(format!(
            "HTTP {status} from {url}: {body}"
        )));
    }
    if status.is_server_error() {
        return Some(LlmError::RequestFailed(format!(
            "HTTP {status} from {url}: {body}"
        )));
    }
    None
}
