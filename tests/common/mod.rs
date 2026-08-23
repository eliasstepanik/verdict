//! Shared test utilities and mock implementations
#![allow(dead_code, unused_imports)]

pub mod delegation;

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};
use verdict::prelude::*;

/// Mock LLM provider for testing
#[derive(Clone)]
pub struct MockLlmProvider {
    pub expected_response: String,
    pub captured_request: Arc<Mutex<Option<LlmRequest>>>,
}

impl MockLlmProvider {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            expected_response: response.into(),
            captured_request: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        *self.captured_request.lock().unwrap() = Some(req);
        Ok(LlmResponse {
            content: self.expected_response.clone(),
            model: "mock".into(),
            usage: None,
            tool_calls: None,
        })
    }

    fn stream(
        &self,
        request: LlmRequest,
    ) -> std::pin::Pin<
        Box<
            dyn futures::stream::Stream<Item = Result<verdict::LlmChunk, verdict::LlmError>> + Send,
        >,
    > {
        let response = self.expected_response.clone();
        *self.captured_request.lock().unwrap() = Some(request);
        Box::pin(futures::stream::once(async move {
            Ok(verdict::LlmChunk {
                delta: response,
                finish_reason: Some("stop".to_string()),
            })
        }))
    }
}

/// A single scripted LLM response (or error)
pub enum ScriptedResponse {
    /// Successful response with text and optional tool calls
    Success {
        content: String,
        tool_calls: Option<Vec<(String, serde_json::Value)>>, // (tool_name, args)
        usage: Option<LlmUsage>,
    },
    /// Failure response — LLM provider returns an error
    Error(LlmError),
}

impl ScriptedResponse {
    /// Create a text-only response
    pub fn text(content: impl Into<String>) -> Self {
        Self::Success {
            content: content.into(),
            tool_calls: None,
            usage: None,
        }
    }

    /// Create a response with a single tool call
    pub fn tool_call(tool_name: impl Into<String>, args: serde_json::Value) -> Self {
        Self::Success {
            content: String::new(),
            tool_calls: Some(vec![(tool_name.into(), args)]),
            usage: None,
        }
    }

    /// Create a response with multiple tool calls
    pub fn multi_tool_call(calls: Vec<(String, serde_json::Value)>) -> Self {
        Self::Success {
            content: String::new(),
            tool_calls: Some(calls),
            usage: None,
        }
    }

    /// Create a response with usage info (for cost tracking tests)
    pub fn with_usage(
        content_or_tool: impl Into<String>,
        args_or_empty: impl Into<serde_json::Value>,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) -> Self {
        let content_str = content_or_tool.into();
        let args_val = args_or_empty.into();
        let (content, tool_calls) = if args_val.is_object() && !args_val.as_object().unwrap().is_empty() {
            // It's a tool call
            (String::new(), Some(vec![(content_str, args_val)]))
        } else {
            // It's text
            (content_str, None)
        };
        
        Self::Success {
            content,
            tool_calls,
            usage: Some(LlmUsage {
                prompt_tokens,
                completion_tokens,
            }),
        }
    }

    /// Create an error response — synthesizes an LlmError
    pub fn error(llm_error: LlmError) -> Self {
        Self::Error(llm_error)
    }
}

/// A scripted mock LLM provider that returns different responses in sequence.
/// Each call to `complete()` returns the next response in the script.
/// The script can include tool calls (via tool_calls field) or plain text.
///
/// CRITICAL: This provider CAPTURES all incoming requests in order, allowing
/// tests to inspect the conversation history (which contains potentially redacted
/// or raw tool results).
pub struct ScriptedMockLlmProvider {
    /// Pre-programmed responses, returned in order.
    pub responses: Vec<ScriptedResponse>,
    /// Index of the next response to return.
    pub call_index: AtomicUsize,
    /// Captured incoming LlmRequest objects, one per complete() call
    pub captured_requests: Arc<Mutex<Vec<LlmRequest>>>,
}

impl ScriptedMockLlmProvider {
    pub fn new(responses: Vec<ScriptedResponse>) -> Self {
        Self {
            responses,
            call_index: AtomicUsize::new(0),
            captured_requests: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl LlmProvider for ScriptedMockLlmProvider {
    fn name(&self) -> &str {
        "scripted-mock"
    }

    fn default_model(&self) -> &str {
        "scripted-mock-model"
    }

    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // CRITICAL: Capture the incoming request so tests can inspect the conversation history
        self.captured_requests.lock().unwrap().push(req.clone());
        
        let idx = self.call_index.fetch_add(1, Ordering::SeqCst);
        if idx >= self.responses.len() {
            // Out of script — return a default text response
            return Ok(LlmResponse {
                content: "Done.".to_string(),
                model: "scripted-mock".into(),
                usage: None,
                tool_calls: None,
            });
        }
        let r = &self.responses[idx];
        match r {
            ScriptedResponse::Error(LlmError::NetworkError(msg)) => {
                Err(LlmError::NetworkError(msg.clone()))
            }
            ScriptedResponse::Error(LlmError::RequestFailed(msg)) => {
                Err(LlmError::RequestFailed(msg.clone()))
            }
            ScriptedResponse::Error(LlmError::InvalidResponse(msg)) => {
                Err(LlmError::InvalidResponse(msg.clone()))
            }
            ScriptedResponse::Error(LlmError::RateLimited) => {
                Err(LlmError::RateLimited)
            }
            ScriptedResponse::Error(LlmError::AuthFailed) => {
                Err(LlmError::AuthFailed)
            }
            ScriptedResponse::Error(LlmError::NotConfigured) => {
                Err(LlmError::NotConfigured)
            }
            ScriptedResponse::Success { content, tool_calls, usage } => {
                let tool_calls = tool_calls.as_ref().map(|calls| {
                    calls
                        .iter()
                        .enumerate()
                        .map(|(i, (name, args))| ToolCall {
                            name: name.clone(),
                            arguments: args.clone(),
                            id: Some(format!("call_{}", i)),
                        })
                        .collect::<Vec<_>>()
                });
                Ok(LlmResponse {
                    content: content.clone(),
                    model: "scripted-mock".into(),
                    usage: usage.clone(),
                    tool_calls,
                })
            }
        }
    }

    fn stream(
        &self,
        _req: LlmRequest,
    ) -> std::pin::Pin<
        Box<
            dyn futures::stream::Stream<Item = Result<verdict::LlmChunk, verdict::LlmError>> + Send,
        >,
    > {
        Box::pin(futures::stream::once(async {
            Ok(verdict::LlmChunk {
                delta: "Done.".into(),
                finish_reason: Some("stop".into()),
            })
        }))
    }
}

/// Create a dummy McpClient for tests that don't actually call MCP tools
/// This is a minimal config with no command or URL, so it won't spawn processes or make HTTP calls
pub async fn create_dummy_mcp_client(
) -> Result<verdict::mcp::client::McpClient, verdict::mcp::client::McpError> {
    let config = McpServerConfig::new("test_dummy");
    verdict::mcp::client::McpClient::connect(config).await
}

/// A test tool that returns a response containing a fake API key.
/// Used to verify that ToolUseLoop's redaction gates correctly redact (Strict) or allow (None) secrets.
pub struct SecretBearingTestTool;

impl SecretBearingTestTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl verdict::tools::Tool for SecretBearingTestTool {
    fn name(&self) -> &str {
        "test_tool"
    }
    
    fn description(&self) -> &str {
        "A test tool that returns a secret-bearing result"
    }
    
    fn schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "arg": { "type": "string" }
            }
        })
    }
    
    fn source(&self) -> verdict::tools::ToolSource {
        verdict::tools::ToolSource::Builtin
    }
    
    async fn call(
        &self,
        _args: serde_json::Value,
        _ctx: verdict::tools::ToolContext,
    ) -> Result<verdict::tools::ToolOutput, verdict::tools::ToolError> {
        // Return a response containing a fake secret (longer key to pass scanner's >20 char threshold)
        Ok(verdict::tools::ToolOutput::text(
            "Test tool called. Debug info: sk-proj-fake1234567890abcdefghij. Done.".to_string()
        ))
    }
}
