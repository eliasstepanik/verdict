//! Shared test utilities and mock implementations
#![allow(dead_code, unused_imports)]

use std::sync::Mutex;
use verdict::prelude::*;
use async_trait::async_trait;
use std::sync::Arc;

/// Mock LLM provider for testing
pub struct MockLlmProvider {
    pub expected_response: String,
    pub captured_request: Mutex<Option<LlmRequest>>,
}

impl MockLlmProvider {
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            expected_response: response.into(),
            captured_request: Mutex::new(None),
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
    ) -> std::pin::Pin<Box<dyn futures::stream::Stream<Item = Result<verdict::LlmChunk, verdict::LlmError>> + Send>> {
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

/// Create a dummy McpClient for tests that don't actually call MCP tools
/// This is a minimal config with no command or URL, so it won't spawn processes or make HTTP calls
pub async fn create_dummy_mcp_client() -> Result<verdict::mcp::client::McpClient, verdict::mcp::client::McpError> {
    let config = McpServerConfig::new("test_dummy");
    verdict::mcp::client::McpClient::connect(config).await
}
