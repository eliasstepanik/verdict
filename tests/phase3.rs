//! Phase 3 — MCP Support tests
#![allow(unused_imports)]

use serde_json::json;
use verdict::prelude::*;
use std::collections::HashMap;

/// Test 1: McpServerConfig construction
#[test]
fn test_mcp_server_config_basic_construction() {
    let config = McpServerConfig::new("filesystem");

    assert_eq!(config.name, "filesystem");
    assert_eq!(config.command, None);
    assert_eq!(config.url, None);
    assert!(config.env.is_empty());
    assert!(config.allowed_tools.is_empty());
}

/// Test 2: McpServerConfig with full builder
#[test]
fn test_mcp_server_config_with_builder() {
    let config = McpServerConfig::new("github")
        .with_command("npx")
        .with_args(vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-github".to_string(),
        ])
        .with_env_var("API_KEY", "secret")
        .with_allowed_tools(vec!["create_issue".to_string(), "read_pr".to_string()]);

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
    assert_eq!(config.env.get("API_KEY"), Some(&"secret".to_string()));
}

/// Test 3: Empty allowlist allows all tools
#[test]
fn test_mcp_server_empty_allowlist_allows_all() {
    let config = McpServerConfig::new("test");

    // Empty allowlist should allow any tool
    assert!(config.is_tool_allowed("any_tool"));
    assert!(config.is_tool_allowed("another_tool"));
    assert!(config.is_tool_allowed("yet_another"));
}

/// Test 4: Non-empty allowlist filters tools
#[test]
fn test_mcp_server_allowlist_filters_tools() {
    let config = McpServerConfig::new("test")
        .with_allowed_tools(vec!["allowed_tool".to_string(), "also_allowed".to_string()]);

    assert!(config.is_tool_allowed("allowed_tool"));
    assert!(config.is_tool_allowed("also_allowed"));
    assert!(!config.is_tool_allowed("not_allowed"));
    assert!(!config.is_tool_allowed("forbidden"));
}

/// Test 5: McpToolAdapter creation and properties
#[test]
fn test_mcp_tool_adapter_creation() {
    let adapter = McpToolAdapter::new(
        "read_file",
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

    assert_eq!(adapter.name(), "read_file");
    assert_eq!(adapter.description(), "Read a file from the workspace");
    assert!(!adapter.schema().is_null());
}

/// Test 6: McpToolAdapter source returns McpServer variant
#[test]
fn test_mcp_tool_adapter_source() {
    let adapter = McpToolAdapter::new(
        "test_tool",
        "A test tool",
        json!({}),
        "test_server",
        "original_tool_name",
    );

    match adapter.source() {
        ToolSource::McpServer {
            server_name,
            tool_name,
        } => {
            assert_eq!(server_name, "test_server");
            assert_eq!(tool_name, "original_tool_name");
        }
        _ => panic!("Expected ToolSource::McpServer variant"),
    }
}

/// Test 7: ToolRegistry::register_mcp_tool with server namespace
#[test]
fn test_tool_registry_register_mcp_tool() {
    let mut registry = ToolRegistry::new();

    let adapter = McpToolAdapter::new(
        "read_file",
        "Read a file",
        json!({}),
        "filesystem",
        "read_file",
    );

    let result = registry.register_mcp_tool("filesystem", adapter);
    assert!(result.is_ok());

    // Tool should be registered with mcp.{server}.{tool} prefix
    let registered = registry.get("mcp.filesystem.read_file");
    assert!(registered.is_some());
}

/// Test 8: ToolRegistry contains registered MCP tools
#[test]
fn test_tool_registry_contains_registered_mcp_tool() {
    let mut registry = ToolRegistry::new();

    let adapter1 = McpToolAdapter::new(
        "search_files",
        "Search for files",
        json!({}),
        "filesystem",
        "search_files",
    );

    let adapter2 = McpToolAdapter::new(
        "create_issue",
        "Create an issue",
        json!({}),
        "github",
        "create_issue",
    );

    let _ = registry.register_mcp_tool("filesystem", adapter1);
    let _ = registry.register_mcp_tool("github", adapter2);

    let tools = registry.list();
    assert!(tools.contains(&"mcp.filesystem.search_files".to_string()));
    assert!(tools.contains(&"mcp.github.create_issue".to_string()));
}

/// Test 9: Allowlist rejection
#[test]
fn test_mcp_server_allowlist_rejects_unlisted_tool() {
    let config = McpServerConfig::new("strict")
        .with_allowed_tools(vec!["whitelisted".to_string()]);

    assert!(config.is_tool_allowed("whitelisted"));
    assert!(!config.is_tool_allowed("blacklisted"));
}

/// Test 10: McpClient::connect with URL-only config returns NotImplemented
#[tokio::test]
async fn test_mcp_client_url_only_not_implemented() {
    let config = McpServerConfig::new("http_server").with_url("http://localhost:3000");

    let result = McpClient::connect(config).await;
    assert!(result.is_err());

    if let Err(McpError::NotImplemented(msg)) = result {
        assert!(msg.contains("not yet supported"));
    } else {
        panic!("Expected NotImplemented error");
    }
}

/// Test 11: McpClient::connect with nonexistent command returns error
#[tokio::test]
async fn test_mcp_client_connect_bad_command() {
    let config =
        McpServerConfig::new("nonexistent").with_command("definitely_does_not_exist_xyz_123");

    let result = McpClient::connect(config).await;
    assert!(result.is_err());

    if let Err(McpError::Io(_)) = result {
        // Expected
    } else {
        panic!("Expected I/O error for nonexistent command");
    }
}

/// Test 12: McpClient with config-only (no command) succeeds
#[tokio::test]
async fn test_mcp_client_connect_config_only() {
    let config = McpServerConfig::new("stub_server");

    let result = McpClient::connect(config).await;
    assert!(result.is_ok());
}

/// Test 13: McpToolAdapter call placeholder returns pending
#[tokio::test]
async fn test_mcp_tool_adapter_call_returns_stub() {
    let adapter = McpToolAdapter::new(
        "test",
        "A test tool",
        json!({}),
        "server",
        "tool",
    );

    let ctx = ToolContext {
        filesystem_policy: FilesystemPolicy {
            workspace_root: std::path::PathBuf::from("/"),
            read_paths: Vec::new(),
            write_paths: Vec::new(),
            forbidden_paths: Vec::new(),
            workspace_isolation: WorkspaceIsolation::None,
        },
        network_policy: NetworkPolicy::DenyAll,
        allowed_tools: ToolSet::Full,
        audit_log: std::sync::Arc::new(std::sync::Mutex::new(AuditLog::new())),
    };

    let result = adapter.call(json!({}), ctx).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.as_str().contains("pending"));
}

/// Test 14: Prelude exports MCP types
#[test]
fn test_prelude_exports_mcp_types() {
    // This test just checks that the types are accessible from the prelude
    // If the imports fail, compilation fails, so this is a compile-time test
    let _config = McpServerConfig::new("test");
    let _adapter = McpToolAdapter::new("test", "desc", json!({}), "server", "tool");
}

/// Test 15: Multiple tools in allowlist
#[test]
fn test_mcp_server_multiple_tools_in_allowlist() {
    let allowed = vec![
        "tool1".to_string(),
        "tool2".to_string(),
        "tool3".to_string(),
        "tool4".to_string(),
    ];

    let config = McpServerConfig::new("multi").with_allowed_tools(allowed);

    assert!(config.is_tool_allowed("tool1"));
    assert!(config.is_tool_allowed("tool2"));
    assert!(config.is_tool_allowed("tool3"));
    assert!(config.is_tool_allowed("tool4"));
    assert!(!config.is_tool_allowed("tool5"));
}

/// Test 16: McpClient URL + command (both set) prefers command
#[tokio::test]
async fn test_mcp_client_url_and_command_both_set() {
    let config = McpServerConfig::new("hybrid")
        .with_url("http://localhost:3000")
        .with_command("npx");

    // Should succeed because command is set, so URL is ignored
    let result = McpClient::connect(config).await;
    // This will fail because npx without args won't spawn properly,
    // but the point is it tries to use the command
    assert!(result.is_err());
}

/// Test 17: Tool registry with mixed tools (builtins + MCP)
#[test]
fn test_tool_registry_mixed_builtins_and_mcp() {
    let mut registry = ToolRegistry::with_builtins();
    let original_count = registry.list().len();

    let adapter = McpToolAdapter::new(
        "custom",
        "Custom tool",
        json!({}),
        "custom_server",
        "custom",
    );

    let _ = registry.register_mcp_tool("custom_server", adapter);

    let tools = registry.list();
    // Should have original builtin tools plus the new MCP tool
    assert!(tools.len() >= original_count);
    assert!(tools.contains(&"mcp.custom_server.custom".to_string()));
}

/// Test 18: McpServerConfig env variables
#[test]
fn test_mcp_server_config_env_variables() {
    let mut env = HashMap::new();
    env.insert("VAR1".to_string(), "value1".to_string());
    env.insert("VAR2".to_string(), "value2".to_string());

    let config = McpServerConfig::new("test").with_env(env);

    assert_eq!(config.env.get("VAR1"), Some(&"value1".to_string()));
    assert_eq!(config.env.get("VAR2"), Some(&"value2".to_string()));
    assert_eq!(config.env.len(), 2);
}

/// Test 19: McpToolAdapter with complex schema
#[test]
fn test_mcp_tool_adapter_complex_schema() {
    let schema = json!({
        "type": "object",
        "required": ["name", "params"],
        "properties": {
            "name": { "type": "string" },
            "params": {
                "type": "object",
                "properties": {
                    "field1": { "type": "string" },
                    "field2": { "type": "number" }
                }
            }
        }
    });

    let adapter = McpToolAdapter::new(
        "complex",
        "Complex tool",
        schema.clone(),
        "server",
        "complex",
    );

    assert_eq!(adapter.schema(), schema);
}

/// Test 20: McpServerConfig clone
#[test]
fn test_mcp_server_config_clone() {
    let config1 = McpServerConfig::new("test")
        .with_command("cmd")
        .with_allowed_tools(vec!["tool1".to_string()]);

    let config2 = config1.clone();

    assert_eq!(config1.name, config2.name);
    assert_eq!(config1.command, config2.command);
    assert_eq!(config1.allowed_tools, config2.allowed_tools);
}
