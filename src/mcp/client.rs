//! MCP client for connecting to MCP servers and discovering tools
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use thiserror::Error;
use tokio::process::{Child, Command};

use super::server::McpServerConfig;

/// Error type for MCP operations
#[derive(Error, Debug, Clone)]
pub enum McpError {
    /// MCP server not running
    #[error("MCP server not running")]
    NotRunning,

    /// JSON-RPC error
    #[error("JSON-RPC error: {0}")]
    JsonRpc(String),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(String),

    /// Not implemented (URL-based servers in Phase 3)
    #[error("not implemented: {0}")]
    NotImplemented(String),

    /// Tool not found
    #[error("tool not found: {0}")]
    ToolNotFound(String),

    /// Tool rejected by allowlist
    #[error("tool not allowed: {0}")]
    AllowlistRejected(String),

    /// Invalid tool definition
    #[error("invalid tool definition: {0}")]
    InvalidToolDef(String),
}

/// Tool definition discovered from an MCP server
#[derive(Debug, Clone)]
pub struct DiscoveredTool {
    /// Tool name
    pub name: String,

    /// Tool description
    pub description: String,

    /// JSON Schema for input arguments
    pub input_schema: Value,
}

/// MCP client for communicating with MCP servers
pub struct McpClient {
    config: McpServerConfig,
    process: Option<Child>,
    request_id: Arc<Mutex<u64>>,
}

impl McpClient {
    /// Connect to an MCP server
    /// For command-based servers, spawns the process immediately.
    /// For URL-based servers, returns NotImplemented (Phase 7+).
    pub async fn connect(config: McpServerConfig) -> Result<Self, McpError> {
        // URL-based servers not supported in Phase 3
        if config.url.is_some() && config.command.is_none() {
            return Err(McpError::NotImplemented(
                "URL-based MCP servers are not yet supported (Phase 7+)".to_string(),
            ));
        }

        // Spawn the command if present
        let process = if let Some(command) = &config.command {
            let mut cmd = Command::new(command);

            if let Some(args) = &config.args {
                cmd.args(args);
            }

            for (key, value) in &config.env {
                cmd.env(key, value);
            }

            cmd.stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null());

            match cmd.spawn() {
                Ok(child) => Some(child),
                Err(e) => {
                    return Err(McpError::Io(format!(
                        "Failed to spawn '{}': {}",
                        command, e
                    )))
                }
            }
        } else {
            None
        };

        Ok(Self {
            config,
            process,
            request_id: Arc::new(Mutex::new(0)),
        })
    }

    /// Discover tools available from the MCP server
    /// 
    /// In Phase 3, this is a placeholder that returns an empty list.
    /// Full implementation requires maintaining persistent stdio connections,
    /// which will be implemented in Phase 4+ when needed.
    pub async fn discover_tools(&mut self) -> Result<Vec<DiscoveredTool>, McpError> {
        // Phase 3: Return empty list as placeholder
        // Full implementation requires proper async stdio handling
        Ok(Vec::new())
    }

    /// Parse a tool definition from a JSON object
    /// 
    /// Note: Currently unused, but kept for future use in Phase 4+ when
    /// full JSON-RPC communication is implemented.
    #[allow(dead_code)]
    fn parse_tool_definition(&self, tool_def: &Value) -> Result<DiscoveredTool, McpError> {
        let name = tool_def
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::InvalidToolDef("Missing 'name' field".to_string()))?
            .to_string();

        let description = tool_def
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();

        let input_schema = tool_def
            .get("inputSchema")
            .cloned()
            .unwrap_or(json!({}));

        Ok(DiscoveredTool {
            name,
            description,
            input_schema,
        })
    }

    /// Call a tool on the MCP server
    /// 
    /// In Phase 3, this is a placeholder.
    pub async fn call_tool(
        &mut self,
        _tool_name: &str,
        _arguments: Value,
    ) -> Result<Value, McpError> {
        if self.process.is_none() {
            return Err(McpError::NotRunning);
        }

        // Phase 3: Not yet implemented
        Err(McpError::NotImplemented(
            "Tool calls not yet implemented in Phase 3".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mcp_client_connect_nonexistent_command() {
        let config = McpServerConfig::new("nonexistent")
            .with_command("nonexistent_command_xyz");

        let result = McpClient::connect(config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_client_connect_url_only_not_implemented() {
        let config = McpServerConfig::new("http_server").with_url("http://localhost:8080");

        let result = McpClient::connect(config).await;
        assert!(result.is_err());
        if let Err(McpError::NotImplemented(_)) = result {
            // Expected
        } else {
            panic!("Expected NotImplemented error");
        }
    }

    #[tokio::test]
    async fn test_mcp_client_discover_tools_not_running() {
        let config = McpServerConfig::new("test");
        let mut client = McpClient::connect(config).await.unwrap();

        let result = client.discover_tools().await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
