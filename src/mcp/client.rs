//! MCP client for connecting to MCP servers and discovering tools
use reqwest::Client as ReqwestClient;
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
    http_client: Option<ReqwestClient>,
    base_url: Option<String>,
    request_id: Arc<Mutex<u64>>,
}

impl McpClient {
    /// Connect to an MCP server
    /// For command-based servers, spawns the process immediately.
    /// For URL-based servers, stores the URL for HTTP communication.
    pub async fn connect(config: McpServerConfig) -> Result<Self, McpError> {
        // Handle URL-only servers (HTTP transport in Phase 12)
        if config.url.is_some() && config.command.is_none() {
            let url = config.url.clone().unwrap();
            return Ok(Self {
                config,
                process: None,
                http_client: Some(ReqwestClient::new()),
                base_url: Some(url),
                request_id: Arc::new(Mutex::new(0)),
            });
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
            http_client: None,
            base_url: None,
            request_id: Arc::new(Mutex::new(0)),
        })
    }

    /// Discover tools available from the MCP server
    pub async fn discover_tools(&mut self) -> Result<Vec<DiscoveredTool>, McpError> {
        // Handle HTTP-based servers (Phase 12)
        if let (Some(http_client), Some(base_url)) = (&self.http_client, &self.base_url) {
            let req_body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
            let response: Value = http_client
                .post(format!("{}/tools/list", base_url.trim_end_matches('/')))
                .header("Content-Type", "application/json")
                .json(&req_body)
                .send()
                .await
                .map_err(|e| McpError::Io(e.to_string()))?
                .json()
                .await
                .map_err(|e| McpError::JsonRpc(e.to_string()))?;
            
            let tools_arr = response.get("result")
                .and_then(|r| r.get("tools"))
                .and_then(|t| t.as_array())
                .ok_or_else(|| McpError::JsonRpc("missing 'result.tools'".into()))?;
            
            let mut discovered = Vec::new();
            for tool_def in tools_arr {
                let name = tool_def.get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| McpError::InvalidToolDef("missing 'name'".into()))?
                    .to_string();
                let description = tool_def.get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input_schema = tool_def.get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                
                if self.config.allowed_tools.is_empty() || self.config.allowed_tools.contains(&name) {
                    discovered.push(DiscoveredTool { name, description, input_schema });
                }
            }
            return Ok(discovered);
        }

        // Check that child process is running
        if self.process.is_none() {
            return Err(McpError::NotRunning);
        }

        // Get a reference to stdin and stdout from the child process
        let process = self.process.as_mut().ok_or(McpError::NotRunning)?;
        let stdin = process.stdin.as_mut().ok_or(McpError::Io("no stdin".into()))?;
        let stdout = process.stdout.as_mut().ok_or(McpError::Io("no stdout".into()))?;

        // Increment ID counter
        let id = {
            let mut id_ref = self.request_id.lock().map_err(|_| {
                McpError::JsonRpc("failed to acquire request ID lock".to_string())
            })?;
            *id_ref += 1;
            *id_ref
        };

        // Write JSON-RPC request
        use tokio::io::AsyncWriteExt;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/list",
            "params": {}
        });
        let request_str = format!("{}\n", request.to_string());
        stdin
            .write_all(request_str.as_bytes())
            .await
            .map_err(|e| McpError::Io(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Io(e.to_string()))?;

        // Read response using BufReader
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .await
            .map_err(|e| McpError::Io(e.to_string()))?;

        // Parse JSON-RPC response
        let response: Value = serde_json::from_str(&response_line)
            .map_err(|e| McpError::JsonRpc(e.to_string()))?;

        // Extract tools from result.tools
        let tools = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| McpError::JsonRpc("missing or invalid 'tools' field".into()))?;

        let mut discovered = Vec::new();
        for tool_def in tools {
            match self.parse_tool_definition(tool_def) {
                Ok(tool) => {
                    // Apply allowed_tools filter if configured
                    if self.config.allowed_tools.is_empty()
                        || self.config.allowed_tools.contains(&tool.name)
                    {
                        discovered.push(tool);
                    }
                }
                Err(e) => {
                    // Skip invalid tools with a log (in real implementation)
                    eprintln!("Warning: skipping invalid tool definition: {}", e);
                }
            }
        }

        Ok(discovered)
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
    pub async fn call_tool(
        &mut self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, McpError> {
        // Handle HTTP-based servers (Phase 12)
        if let (Some(http_client), Some(base_url)) = (&self.http_client, &self.base_url) {
            let id = {
                let mut id_ref = self.request_id.lock().map_err(|_| McpError::JsonRpc("lock poisoned".to_string()))?;
                *id_ref += 1;
                *id_ref
            };
            
            let req_body = json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": {"name": tool_name, "arguments": arguments}
            });
            
            let response: Value = http_client
                .post(format!("{}/tools/call", base_url.trim_end_matches('/')))
                .header("Content-Type", "application/json")
                .json(&req_body)
                .send()
                .await
                .map_err(|e| McpError::Io(e.to_string()))?
                .json()
                .await
                .map_err(|e| McpError::JsonRpc(e.to_string()))?;
            
            if let Some(error) = response.get("error") {
                let msg = error.get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                return Err(McpError::JsonRpc(format!("tool call failed: {}", msg)));
            }
            
            let content = response.get("result")
                .and_then(|r| r.get("content"))
                .cloned()
                .ok_or_else(|| McpError::JsonRpc("missing 'result.content'".into()))?;
            
            return Ok(content);
        }

        // Check that child process is running
        if self.process.is_none() {
            return Err(McpError::NotRunning);
        }

        // Increment ID counter
        let id = {
            let mut id_ref = self.request_id.lock().ok().ok_or_else(|| {
                McpError::JsonRpc("failed to acquire request ID lock".to_string())
            })?;
            *id_ref += 1;
            *id_ref
        };

        // Build JSON-RPC request
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {
                "name": tool_name,
                "arguments": arguments
            }
        });

        // Write request to child stdin
        let process = self.process.as_mut().ok_or(McpError::NotRunning)?;
        let stdin = process.stdin.as_mut().ok_or(McpError::Io("no stdin".into()))?;

        use tokio::io::AsyncWriteExt;
        let request_str = format!("{}\n", request.to_string());
        stdin
            .write_all(request_str.as_bytes())
            .await
            .map_err(|e| McpError::Io(e.to_string()))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Io(e.to_string()))?;

        // Read response from child stdout
        let stdout = process.stdout.as_mut().ok_or(McpError::Io("no stdout".into()))?;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .await
            .map_err(|e| McpError::Io(e.to_string()))?;

        // Parse JSON-RPC response
        let response: Value = serde_json::from_str(&response_line)
            .map_err(|e| McpError::JsonRpc(e.to_string()))?;

        // Check for error in response
        if let Some(error) = response.get("error") {
            let error_msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(McpError::JsonRpc(format!("tool call failed: {}", error_msg)));
        }

        // Extract result.content
        let content = response
            .get("result")
            .and_then(|r| r.get("content"))
            .cloned()
            .ok_or_else(|| McpError::JsonRpc("missing 'result.content'".into()))?;

        Ok(content)
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
    async fn test_mcp_client_connect_url_only_now_works() {
        let config = McpServerConfig::new("http_server").with_url("http://localhost:8080");

        let result = McpClient::connect(config).await;
        // Phase 12: URL-only servers are now supported
        assert!(result.is_ok(), "URL-only connect should succeed in Phase 12");
    }

    #[tokio::test]
    async fn test_mcp_client_discover_tools_not_running() {
        let config = McpServerConfig::new("test");
        let mut client = McpClient::connect(config).await.unwrap();

        let result = client.discover_tools().await;
        // Should fail because no process is running
        assert!(result.is_err());
        assert!(matches!(result, Err(McpError::NotRunning)));
    }
}
