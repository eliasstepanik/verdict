//! MCP server configuration
use std::collections::HashMap;

/// Configuration for an MCP server
#[derive(Debug, Clone)]
pub struct McpServerConfig {
    /// Name of the server
    pub name: String,
    
    /// Command to spawn the server (e.g., "npx")
    pub command: Option<String>,
    
    /// Arguments to the command (e.g., ["-y", "@modelcontextprotocol/server-filesystem"])
    pub args: Option<Vec<String>>,
    
    /// URL to connect to (for HTTP/WebSocket-based MCP servers)
    pub url: Option<String>,
    
    /// Environment variables to set when spawning the process
    pub env: HashMap<String, String>,
    
    /// Allowlist of tool names from this server
    /// If empty, all discovered tools are allowed
    pub allowed_tools: Vec<String>,
}

impl McpServerConfig {
    /// Create a new MCP server configuration
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            command: None,
            args: None,
            url: None,
            env: HashMap::new(),
            allowed_tools: Vec::new(),
        }
    }

    /// Set the command to spawn the server
    pub fn with_command(mut self, command: impl Into<String>) -> Self {
        self.command = Some(command.into());
        self
    }

    /// Set the arguments to the command
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = Some(args);
        self
    }

    /// Set the URL for the server
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    /// Set environment variables
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Set the allowlist of tools
    pub fn with_allowed_tools(mut self, allowed_tools: Vec<String>) -> Self {
        self.allowed_tools = allowed_tools;
        self
    }

    /// Add an environment variable
    pub fn with_env_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(key.into(), value.into());
        self
    }

    /// Check if a tool is allowed
    /// Returns true if the allowlist is empty (all tools allowed) or if the tool is in the list
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        if self.allowed_tools.is_empty() {
            return true;
        }
        self.allowed_tools.contains(&tool_name.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_server_config_creation() {
        let config = McpServerConfig::new("filesystem");
        assert_eq!(config.name, "filesystem");
        assert_eq!(config.command, None);
        assert_eq!(config.url, None);
        assert!(config.env.is_empty());
        assert!(config.allowed_tools.is_empty());
    }

    #[test]
    fn test_mcp_server_config_with_builder() {
        let config = McpServerConfig::new("github")
            .with_command("npx")
            .with_args(vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-github".to_string(),
            ])
            .with_allowed_tools(vec![
                "create_issue".to_string(),
                "read_pull_request".to_string(),
            ]);

        assert_eq!(config.name, "github");
        assert_eq!(config.command, Some("npx".to_string()));
        assert_eq!(
            config.args,
            Some(vec![
                "-y".to_string(),
                "@modelcontextprotocol/server-github".to_string(),
            ])
        );
        assert_eq!(config.allowed_tools.len(), 2);
    }

    #[test]
    fn test_empty_allowlist_allows_all() {
        let config = McpServerConfig::new("test");
        assert!(config.is_tool_allowed("any_tool"));
        assert!(config.is_tool_allowed("another_tool"));
    }

    #[test]
    fn test_non_empty_allowlist_filters() {
        let config = McpServerConfig::new("test")
            .with_allowed_tools(vec!["allowed".to_string(), "also_allowed".to_string()]);

        assert!(config.is_tool_allowed("allowed"));
        assert!(config.is_tool_allowed("also_allowed"));
        assert!(!config.is_tool_allowed("not_allowed"));
    }
}
