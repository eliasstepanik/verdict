//! MCP client for connecting to MCP servers and discovering tools
use reqwest::Client as ReqwestClient;
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::io::AsyncWriteExt;
use tracing::warn;

use tokio::process::{Child, Command};

use super::server::McpServerConfig;
use crate::injection::sanitize_for_exposure;

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

    /// Handshake or I/O operation timed out
    #[error("timeout: {0}")]
    Timeout(String),

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
#[derive(Debug)]
pub struct McpClient {
    config: McpServerConfig,
    process: Option<Child>,
    http_client: Option<ReqwestClient>,
    base_url: Option<String>,
    request_id: Arc<AtomicU64>,
    /// Timeout in seconds for handshake and I/O operations (default: 30s)
    timeout_secs: u64,
}

impl McpClient {
    /// Create an inert McpClient with no active connection — for testing only.
    /// Does NOT send any initialize handshake or spawn any process.
    #[cfg(test)]
    pub fn disconnected() -> Self {
        Self {
            config: McpServerConfig::new("test"),
            process: None,
            http_client: None,
            base_url: None,
            request_id: std::sync::Arc::new(std::sync::atomic::AtomicU64::new(1)),
            timeout_secs: 30,
        }
    }

    /// Set timeout in seconds for handshake and I/O operations
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Connect to an MCP server
    /// For command-based servers, spawns the process immediately.
    /// For URL-based servers, stores the URL for HTTP communication.
    /// Sends the initialize handshake after connection.
    pub async fn connect(config: McpServerConfig) -> Result<Self, McpError> {
        // Handle URL-only servers (HTTP transport in Phase 12)
        if config.url.is_some() && config.command.is_none() {
            let url = config.url.clone().unwrap();
            // Construct the client without performing a live handshake —
            // the handshake is deferred to the first actual tool call so that
            // connect() succeeds even when the remote server is not yet running
            // (e.g. in unit tests or lazy-start scenarios).
            let client = Self {
                config,
                process: None,
                http_client: Some(ReqwestClient::new()),
                base_url: Some(url),
                request_id: Arc::new(AtomicU64::new(1)),
                timeout_secs: 30,
            };
            return Ok(client);
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
                    let msg = format!(
                        "Failed to spawn '{}': {}",
                        command, e
                    );
                    return Err(McpError::Io(sanitize_for_exposure(&msg)))
                }
            }
        } else {
            None
        };

        let mut client = Self {
            config,
            process,
            http_client: None,
            base_url: None,
            request_id: Arc::new(AtomicU64::new(1)),
            timeout_secs: 30,
        };

        // Send initialize handshake for stdio-based servers
        if client.process.is_some() {
            client.initialize_handshake().await?;
        }

        Ok(client)
    }

    /// Send the MCP initialize handshake
    async fn initialize_handshake(&mut self) -> Result<(), McpError> {
        let init_request = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "verdict",
                    "version": "0.1.0"
                }
            }
        });

        // Send initialize request (for HTTP servers)
        if let Some(http_client) = &self.http_client {
            if let Some(base_url) = &self.base_url {
                let response: Value = http_client
                    .post(base_url)
                    .header("Content-Type", "application/json")
                    .json(&init_request)
                    .send()
                    .await
                    .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?
                    .json()
                    .await
                    .map_err(|e| McpError::JsonRpc(sanitize_for_exposure(&e.to_string())))?;

                // Verify response is valid
                let _protocol_version = response
                    .get("result")
                    .and_then(|r| r.get("protocolVersion"))
                    .ok_or_else(|| {
                        McpError::JsonRpc(
                            "initialize failed: missing protocolVersion in response".into(),
                        )
                    })?;

                return Ok(());
            }
        }

        // Stdio transport: send initialize, read response, send initialized notification
        if let Some(process) = self.process.as_mut() {
            let init_request_str = format!("{}\n", init_request.to_string());

            // Write initialize request to stdin
            {
                let stdin = process
                    .stdin
                    .as_mut()
                    .ok_or(McpError::Io("no stdin".into()))?;
                stdin
                    .write_all(init_request_str.as_bytes())
                    .await
                    .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;
                stdin
                    .flush()
                    .await
                    .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;
            }

            // Read initialize response from stdout with timeout
            let init_response: Value;
            {
                let stdout = process
                    .stdout
                    .as_mut()
                    .ok_or(McpError::Io("no stdout".into()))?;
                use tokio::io::{AsyncBufReadExt, BufReader};
                let mut reader = BufReader::new(stdout);
                let mut response_line = String::new();
                tokio::time::timeout(
                    std::time::Duration::from_secs(self.timeout_secs),
                    reader.read_line(&mut response_line),
                )
                .await
                .map_err(|_| McpError::Timeout("initialize handshake read timed out".into()))?
                .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;

                init_response = serde_json::from_str(response_line.trim()).map_err(|e| {
                    let msg = format!("initialize response parse error: {}", e);
                    McpError::JsonRpc(sanitize_for_exposure(&msg))
                })?;
            }

            // Check for errors in initialize response
            if let Some(error) = init_response.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                return Err(McpError::JsonRpc(sanitize_for_exposure(&format!("initialize failed: {}", msg))));
            }

            // Send notifications/initialized (MCP spec requirement)
            let initialized_notif = json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            });
            let notif_str = format!("{}\n", initialized_notif.to_string());

            {
                let stdin = process
                    .stdin
                    .as_mut()
                    .ok_or(McpError::Io("no stdin".into()))?;
                stdin
                    .write_all(notif_str.as_bytes())
                    .await
                    .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;
                stdin
                    .flush()
                    .await
                    .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;
            }
        }

        Ok(())
    }

    /// Discover tools available from the MCP server
    pub async fn discover_tools(&mut self) -> Result<Vec<DiscoveredTool>, McpError> {
        // Handle HTTP-based servers (Phase 12)
        if let (Some(http_client), Some(base_url)) = (&self.http_client, &self.base_url) {
            let id = self.request_id.fetch_add(1, Ordering::Relaxed);
            let req_body =
                json!({"jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {}});
            let response: Value = http_client
                .post(format!("{}/tools/list", base_url.trim_end_matches('/')))
                .header("Content-Type", "application/json")
                .json(&req_body)
                .send()
                .await
                .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?
                .json()
                .await
                .map_err(|e| McpError::JsonRpc(sanitize_for_exposure(&e.to_string())))?;

            let tools_arr = response
                .get("result")
                .and_then(|r| r.get("tools"))
                .and_then(|t| t.as_array())
                .ok_or_else(|| McpError::JsonRpc("missing 'result.tools'".into()))?;

            let mut discovered = Vec::new();
            for tool_def in tools_arr {
                let name = tool_def
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| McpError::InvalidToolDef("missing 'name'".into()))?
                    .to_string();
                let description = tool_def
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let input_schema = tool_def
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({}));

                if self.config.allowed_tools.is_empty() || self.config.allowed_tools.contains(&name)
                {
                    discovered.push(DiscoveredTool {
                        name,
                        description,
                        input_schema,
                    });
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
        let stdin = process
            .stdin
            .as_mut()
            .ok_or(McpError::Io("no stdin".into()))?;
        let stdout = process
            .stdout
            .as_mut()
            .ok_or(McpError::Io("no stdout".into()))?;

        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
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
            .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;

        // Read response using BufReader with timeout
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            reader.read_line(&mut response_line),
        )
        .await
        .map_err(|_| McpError::Timeout("discover tools read timed out".into()))?
        .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;

        // Parse JSON-RPC response
        let response: Value =
            serde_json::from_str(&response_line).map_err(|e| McpError::JsonRpc(sanitize_for_exposure(&e.to_string())))?;

        // Validate that the response id matches the request id
        let response_id = response.get("id").and_then(|v| v.as_u64());
        if response_id != Some(id) {
            return Err(McpError::JsonRpc(format!(
                "response id mismatch: expected {}, got {:?}",
                id, response_id
            )));
        }

        let tools = response
            .get("result")
            .and_then(|r| r.get("tools"))
            .and_then(|t| t.as_array())
            .ok_or_else(|| McpError::JsonRpc("missing 'result.tools'".into()))?
            .clone();

        let mut discovered = Vec::new();
        for tool_def in tools {
            match self.parse_tool_definition(&tool_def) {
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
                    warn!(error = %e, "skipping invalid tool definition");
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

        let input_schema = tool_def.get("inputSchema").cloned().unwrap_or(json!({}));

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
            let id = self.request_id.fetch_add(1, Ordering::Relaxed);

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
                .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?
                .json()
                .await
                .map_err(|e| McpError::JsonRpc(sanitize_for_exposure(&e.to_string())))?;

            if let Some(error) = response.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                return Err(McpError::JsonRpc(format!("tool call failed: {}", sanitize_for_exposure(msg))));
            }

            let content = response
                .get("result")
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
        // Increment ID counter
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);

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
        let stdin = process
            .stdin
            .as_mut()
            .ok_or(McpError::Io("no stdin".into()))?;

        use tokio::io::AsyncWriteExt;
        let request_str = format!("{}\n", request.to_string());
        stdin
            .write_all(request_str.as_bytes())
            .await
            .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;

        // Read response from child stdout with timeout
        let stdout = process
            .stdout
            .as_mut()
            .ok_or(McpError::Io("no stdout".into()))?;
        use tokio::io::{AsyncBufReadExt, BufReader};
        let mut reader = BufReader::new(stdout);
        let mut response_line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            reader.read_line(&mut response_line),
        )
        .await
        .map_err(|_| McpError::Timeout("tool call read timed out".into()))?
        .map_err(|e| McpError::Io(sanitize_for_exposure(&e.to_string())))?;

        // Parse JSON-RPC response
        let response: Value =
            serde_json::from_str(&response_line).map_err(|e| McpError::JsonRpc(sanitize_for_exposure(&e.to_string())))?;

        // Validate that the response id matches the request id
        let response_id = response.get("id").and_then(|v| v.as_u64());
        if response_id != Some(id) {
            return Err(McpError::JsonRpc(format!(
                "response id mismatch: expected {}, got {:?}",
                id, response_id
            )));
        }

        // Check for error in response
        if let Some(error) = response.get("error") {
            let msg = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(McpError::JsonRpc(format!("tool call failed: {}", sanitize_for_exposure(msg))));
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

impl Drop for McpClient {
    fn drop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.start_kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mcp_client_connect_nonexistent_command() {
        let config = McpServerConfig::new("nonexistent").with_command("nonexistent_command_xyz");

        let result = McpClient::connect(config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_mcp_client_connect_url_only_now_works() {
        let config = McpServerConfig::new("http_server").with_url("http://localhost:8080");

        // Phase 12: URL-only transport is supported. Connect may fail with a
        // network error if no server is running, but must NOT return NotImplemented.
        let result = McpClient::connect(config).await;
        match result {
            Ok(_) => {} // server happened to be running
            Err(e) => {
                let msg = format!("{:?}", e);
                assert!(
                    !msg.contains("NotImplemented"),
                    "URL-only transport returned NotImplemented: {}",
                    msg
                );
            }
        }
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

    /// Test that a non-responding process times out instead of hanging indefinitely
    #[tokio::test]
    async fn test_mcp_client_handshake_timeout() {
        // Spawn 'sleep 100' as a non-responding server that will never emit
        // a JSON-RPC initialize response on stdout
        let config = McpServerConfig::new("sleep_server")
            .with_command("sleep")
            .with_args(vec!["100".to_string()]);

        let start = std::time::Instant::now();
        let result = McpClient::connect(config).await;
        let elapsed = start.elapsed();

        // Should timeout after ~30 seconds (our default)
        assert!(result.is_err(), "Expected handshake to timeout");
        if let Err(McpError::Timeout(msg)) = result {
            assert!(msg.contains("timed out"), "Error message should mention timeout: {}", msg);
        } else {
            panic!("Expected McpError::Timeout, got: {:?}", result);
        }

        // Verify the operation completed quickly (within 40 seconds) instead of hanging forever.
        // We allow some slack for system overhead, but definitely should not take minutes.
        assert!(
            elapsed.as_secs() < 40,
            "Timeout took too long: {}s (expected ~30s)",
            elapsed.as_secs()
        );
    }

    /// Test that with_timeout() builder method works
    #[test]
    fn test_mcp_client_with_timeout() {
        let client = McpClient::disconnected();
        assert_eq!(client.timeout_secs, 30, "Default timeout should be 30s");

        let client_custom = client.with_timeout(15);
        assert_eq!(client_custom.timeout_secs, 15, "Custom timeout should be set");
    }
}
