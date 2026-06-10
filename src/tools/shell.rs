//! Built-in shell tools

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::process::Command;

use crate::tools::tool::{Tool, ToolOutput, ToolError, ToolSource, ToolContext};

/// cargo check tool
pub struct CargoCheckTool;

#[async_trait]
impl Tool for CargoCheckTool {
    fn name(&self) -> &str {
        "shell.cargo_check"
    }

    fn description(&self) -> &str {
        "Run cargo check in the workspace root"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, _args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let output = Command::new("cargo")
            .arg("check")
            .current_dir(workspace_root)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to execute cargo check: {}", e),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }
}

/// cargo test tool
pub struct CargoTestTool;

#[async_trait]
impl Tool for CargoTestTool {
    fn name(&self) -> &str {
        "shell.cargo_test"
    }

    fn description(&self) -> &str {
        "Run cargo test in the workspace root"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, _args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let output = Command::new("cargo")
            .arg("test")
            .current_dir(workspace_root)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to execute cargo test: {}", e),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }
}

/// cargo fmt tool
pub struct CargoFmtTool;

#[async_trait]
impl Tool for CargoFmtTool {
    fn name(&self) -> &str {
        "shell.cargo_fmt"
    }

    fn description(&self) -> &str {
        "Run cargo fmt in the workspace root"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, _args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let output = Command::new("cargo")
            .arg("fmt")
            .current_dir(workspace_root)
            .output()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to execute cargo fmt: {}", e),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }
}

/// Execute arbitrary command
pub struct RunCommandTool;

#[async_trait]
impl Tool for RunCommandTool {
    fn name(&self) -> &str {
        "shell.run_command"
    }

    fn description(&self) -> &str {
        "Execute an arbitrary shell command (with restrictions)"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to run"
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Command arguments"
                }
            },
            "required": ["command"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'command' field".to_string(),
            })?;

        let cmd_args: Vec<String> = args
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let mut cmd = Command::new(command);
        for arg in cmd_args {
            cmd.arg(arg);
        }
        cmd.current_dir(workspace_root);

        let output = cmd.output().await.map_err(|e| ToolError::ExecutionFailed {
            reason: format!("failed to execute command: {}", e),
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }
}

/// Factory function to create all shell tools
pub fn shell_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(CargoCheckTool),
        Arc::new(CargoTestTool),
        Arc::new(CargoFmtTool),
        Arc::new(RunCommandTool),
    ]
}
