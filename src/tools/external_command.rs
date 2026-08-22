//! External command-line tools that wrap arbitrary CLI binaries

use async_trait::async_trait;
use serde_json::Value;
use tokio::process::Command;

use crate::tools::tool::{Tool, ToolContext, ToolError, ToolOutput, ToolSource};

/// Wraps an arbitrary CLI binary as a Verdict tool
pub struct ExternalCommandTool {
    pub name: String,
    pub description: String,
    pub command: String,
    pub base_args: Vec<String>,
    pub schema: Value,
}

impl ExternalCommandTool {
    /// Create a new external command tool
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        command: impl Into<String>,
        schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            command: command.into(),
            base_args: Vec::new(),
            schema,
        }
    }

    /// Create a new external command tool with base arguments
    pub fn with_base_args(
        name: impl Into<String>,
        description: impl Into<String>,
        command: impl Into<String>,
        base_args: Vec<String>,
        schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            command: command.into(),
            base_args,
            schema,
        }
    }

    /// Extract command arguments from the JSON schema input
    /// Expects either: { "args": ["arg1", "arg2", ...] } or flat command args
    fn extract_args_from_input(args: &Value) -> Result<Vec<String>, ToolError> {
        match args {
            Value::Object(map) => {
                if let Some(Value::Array(arr)) = map.get("args") {
                    let mut result = Vec::new();
                    for val in arr {
                        match val {
                            Value::String(s) => result.push(s.clone()),
                            _ => {
                                return Err(ToolError::SchemaValidationFailed {
                                    reason: "args array contains non-string value".to_string(),
                                });
                            }
                        }
                    }
                    Ok(result)
                } else {
                    // Empty args object
                    Ok(Vec::new())
                }
            }
            Value::Array(arr) => {
                let mut result = Vec::new();
                for val in arr {
                    match val {
                        Value::String(s) => result.push(s.clone()),
                        _ => {
                            return Err(ToolError::SchemaValidationFailed {
                                reason: "args array contains non-string value".to_string(),
                            });
                        }
                    }
                }
                Ok(result)
            }
            _ => Ok(Vec::new()),
        }
    }

    /// Validate command and arguments for workspace safety
    /// Reuses same patterns as shell.rs validate_command_args_for_workspace_safety
    fn validate_command_safety(command: &str, args: &[String]) -> Result<(), ToolError> {
        // Reject absolute paths in command
        if command.starts_with('/') {
            return Err(ToolError::ExecutionFailed {
                reason: format!("external command path contains absolute path (workspace containment violation): '{}'", command),
            });
        }

        // Reject .. in command
        if command.contains("..") {
            return Err(ToolError::ExecutionFailed {
                reason: format!("external command path contains '..' (workspace containment violation): '{}'", command),
            });
        }

        // Validate arguments using same patterns as shell tools
        for arg in args {
            // Reject absolute paths
            if arg.starts_with('/') {
                return Err(ToolError::ExecutionFailed {
                    reason: format!("argument contains absolute path (workspace containment violation): '{}'", arg),
                });
            }

            // Reject .. path segments
            if arg.contains("..") {
                return Err(ToolError::ExecutionFailed {
                    reason: format!("argument contains '..' path segment (workspace containment violation): '{}'", arg),
                });
            }
        }

        Ok(())
    }
}

#[async_trait]
impl Tool for ExternalCommandTool {
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
        ToolSource::ExternalCommand {
            command: self.command.clone(),
        }
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let workspace_root = &ctx.filesystem_policy.workspace_root;

        // Extract args from input
        let mut call_args = Self::extract_args_from_input(&args)?;

        // Validate workspace safety
        Self::validate_command_safety(&self.command, &call_args)?;

        // Build the full argument list: base args + call args
        let mut all_args = self.base_args.clone();
        all_args.append(&mut call_args);

        // Execute the command
        let output = Command::new(&self.command)
            .args(&all_args)
            .current_dir(workspace_root)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to execute command '{}': {}", self.command, e),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // ponytail: exit code checking is optional — if you need strict failure detection,
        // add `if !output.status.success()` here; for now, return both streams regardless
        let combined = if stderr.is_empty() {
            stdout
        } else if stdout.is_empty() {
            stderr
        } else {
            format!("{}\n{}", stdout, stderr)
        };

        Ok(ToolOutput::text(combined))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn test_extract_args_from_object() {
        let input = json!({ "args": ["arg1", "arg2"] });
        let args = ExternalCommandTool::extract_args_from_input(&input).unwrap();
        assert_eq!(args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_extract_args_from_array() {
        let input = json!(["arg1", "arg2"]);
        let args = ExternalCommandTool::extract_args_from_input(&input).unwrap();
        assert_eq!(args, vec!["arg1", "arg2"]);
    }

    #[test]
    fn test_extract_args_empty_object() {
        let input = json!({});
        let args = ExternalCommandTool::extract_args_from_input(&input).unwrap();
        assert_eq!(args, Vec::<String>::new());
    }

    #[test]
    fn test_validate_command_rejects_absolute_path() {
        let result = ExternalCommandTool::validate_command_safety("/bin/echo", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_command_rejects_parent_dir() {
        let result = ExternalCommandTool::validate_command_safety("../evil", &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_rejects_absolute_path() {
        let result =
            ExternalCommandTool::validate_command_safety("echo", &["/etc/passwd".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_args_rejects_parent_dir() {
        let result = ExternalCommandTool::validate_command_safety("echo", &["..".to_string()]);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_command_accepts_safe_relative() {
        let result = ExternalCommandTool::validate_command_safety("echo", &["hello".to_string()]);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_external_command_tool_echo() {
        use crate::agent::FilesystemPolicy;
        use std::sync::Mutex;

        let tool = ExternalCommandTool::new(
            "echo_test",
            "Test echo command",
            "echo",
            json!({
                "type": "object",
                "properties": {
                    "args": {
                        "type": "array",
                        "items": { "type": "string" }
                    }
                }
            }),
        );

        let ctx = ToolContext {
            filesystem_policy: FilesystemPolicy {
                workspace_root: PathBuf::from("/tmp"),
                read_paths: vec![],
                write_paths: vec![],
                forbidden_paths: vec![],
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
            network_policy: crate::agent::NetworkPolicy::DenyAll,
            allowed_tools: crate::toolset::ToolSet::Full,
            audit_log: Arc::new(Mutex::new(crate::audit::AuditLog::new())),
        };

        let args = json!({ "args": ["hello", "world"] });
        let result = tool.call(args, ctx).await;

        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.raw.contains("hello"));
        assert!(output.raw.contains("world"));
    }

    #[tokio::test]
    async fn test_external_command_tool_source() {
        let tool = ExternalCommandTool::new(
            "test_tool",
            "Test tool",
            "test_command",
            json!({}),
        );

        match tool.source() {
            ToolSource::ExternalCommand { command } => {
                assert_eq!(command, "test_command");
            }
            _ => panic!("Expected ExternalCommand source"),
        }
    }

    #[test]
    fn test_external_command_tool_registration() {
        use crate::registry::ToolRegistry;

        let mut registry = ToolRegistry::new();
        let tool = ExternalCommandTool::new(
            "my_echo",
            "Echo tool",
            "echo",
            json!({}),
        );

        registry.register_external_command(tool);

        // Verify tool is registered and retrievable
        let retrieved = registry.get("my_echo");
        assert!(retrieved.is_some());
        let tool = retrieved.unwrap();
        assert_eq!(tool.name(), "my_echo");
    }
}
