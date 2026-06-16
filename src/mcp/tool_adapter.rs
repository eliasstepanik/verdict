//! MCP tool adapter — wraps discovered MCP tools into the Tool trait
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::mcp::client::McpClient;
use crate::tools::{Tool, ToolContext, ToolError, ToolOutput, ToolSource};

/// An MCP tool adapted to the Verdict Tool trait
#[derive(Clone)]
pub struct McpToolAdapter {
    /// Name of the tool
    name: String,

    /// Description of the tool
    description: String,

    /// JSON Schema for input arguments
    schema: Value,

    /// Server name this tool came from
    server_name: String,

    /// Original tool name from the MCP server
    tool_name: String,

    /// Shared MCP client for calling the tool
    client: Arc<Mutex<McpClient>>,
}

impl McpToolAdapter {
    /// Create a new MCP tool adapter
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        schema: Value,
        server_name: impl Into<String>,
        tool_name: impl Into<String>,
        client: Arc<Mutex<McpClient>>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            schema,
            server_name: server_name.into(),
            tool_name: tool_name.into(),
            client,
        }
    }
}

#[async_trait]
impl Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    fn source(&self) -> ToolSource {
        ToolSource::McpServer {
            server_name: self.server_name.clone(),
            tool_name: self.tool_name.clone(),
        }
    }

    async fn call(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        // Acquire lock on the MCP client and call the tool
        let mut client = self.client.lock().await;
        match client.call_tool(&self.tool_name, args).await {
            Ok(result) => Ok(ToolOutput::json(result)),
            Err(crate::mcp::client::McpError::NotRunning) => {
                // Client has no live process or HTTP connection — return a pending stub
                // so callers can detect the not-yet-connected state gracefully.
                Ok(ToolOutput::text("pending".to_string()))
            }
            Err(e) => Err(ToolError::ExecutionFailed {
                reason: format!("MCP tool call failed: {}", e),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_mcp_tool_adapter_creation() {
        // Use disconnected client — no network needed to test adapter structure
        let client = McpClient::disconnected();

        let adapter = McpToolAdapter::new(
            "mcp.filesystem.read_file",
            "Read a file from the workspace",
            json!({
                "type": "object",
                "required": ["path"],
                "properties": {
                    "path": { "type": "string" }
                }
            }),
            "filesystem",
            "read_file",
            Arc::new(Mutex::new(client)),
        );

        assert_eq!(adapter.name(), "mcp.filesystem.read_file");
        assert_eq!(adapter.description(), "Read a file from the workspace");
        assert_eq!(adapter.server_name, "filesystem");
        assert_eq!(adapter.tool_name, "read_file");
    }

    #[test]
    fn test_mcp_tool_adapter_source() {
        // Use disconnected client — no network needed to test adapter source
        let client = McpClient::disconnected();

        let adapter = McpToolAdapter::new(
            "test.tool",
            "A test tool",
            json!({}),
            "test_server",
            "original_name",
            Arc::new(Mutex::new(client)),
        );

        match adapter.source() {
            ToolSource::McpServer {
                server_name,
                tool_name,
            } => {
                assert_eq!(server_name, "test_server");
                assert_eq!(tool_name, "original_name");
            }
            _ => panic!("Expected McpServer source"),
        }
    }
}
