//! MCP tool adapter — wraps discovered MCP tools into the Tool trait
use async_trait::async_trait;
use serde_json::{json, Value};

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
}

impl McpToolAdapter {
    /// Create a new MCP tool adapter
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        schema: Value,
        server_name: impl Into<String>,
        tool_name: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            schema,
            server_name: server_name.into(),
            tool_name: tool_name.into(),
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

    async fn call(&self, _args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        // In Phase 3, we don't have a persistent client connection to the MCP server.
        // The actual tool execution would happen via PipelineRunner which would
        // need to manage MCP client connections. For now, return a stub.
        // Real implementation will be in PipelineRunner's tool call handling.
        Ok(ToolOutput::json(json!({
            "status": "pending",
            "message": "MCP tool call intercepted but not yet implemented"
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_adapter_creation() {
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
        );

        assert_eq!(adapter.name(), "mcp.filesystem.read_file");
        assert_eq!(adapter.description(), "Read a file from the workspace");
        assert_eq!(adapter.server_name, "filesystem");
        assert_eq!(adapter.tool_name, "read_file");
    }

    #[test]
    fn test_mcp_tool_adapter_source() {
        let adapter = McpToolAdapter::new(
            "test.tool",
            "A test tool",
            json!({}),
            "test_server",
            "original_name",
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

    #[tokio::test]
    async fn test_mcp_tool_adapter_call() {
        let adapter = McpToolAdapter::new(
            "test.tool",
            "A test tool",
            json!({}),
            "test_server",
            "original_name",
        );

        let ctx = ToolContext {
            filesystem_policy: crate::agent::FilesystemPolicy {
                workspace_root: std::path::PathBuf::from("/"),
                read_paths: Vec::new(),
                write_paths: Vec::new(),
                forbidden_paths: Vec::new(),
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
            network_policy: crate::agent::NetworkPolicy::DenyAll,
            allowed_tools: crate::toolset::ToolSet::Full,
            audit_log: std::sync::Arc::new(std::sync::Mutex::new(
                crate::audit::AuditLog::new(),
            )),
        };

        let result = adapter.call(json!({}), ctx).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.as_str().contains("pending"));
    }
}
