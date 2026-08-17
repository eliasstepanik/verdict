#![allow(dead_code)]

//! Built-in shell tools

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::tools::tool::{Tool, ToolChunk, ToolContext, ToolError, ToolOutput, ToolSource};

/// Validate shell command arguments for unsafe patterns.
/// Does NOT prevent OS-level shell escapes (that requires containers/VMs).
/// Guards against naive/accidental escapes via common patterns.
/// Covers:
/// - Literal top-level arguments (absolute paths, .., cd commands)
/// - One level of -c wrapping (bash -c "...", sh -c "...", zsh -c "...")
/// ponytail: Best-effort static validation without a full shell parser.
/// NOT covered: nested -c, base64-encoded commands, variable expansion, eval, ...
/// For adversarial LLM output, OS-level containment (bwrap, namespaces) is needed.
fn validate_command_args_for_workspace_safety(args: &[String]) -> Result<(), ToolError> {
    for (i, arg) in args.iter().enumerate() {
        // Reject absolute paths (start with /)
        if arg.starts_with('/') {
            return Err(ToolError::ExecutionFailed {
                reason: format!("shell command argument contains absolute path (workspace containment violation): '{}'", arg),
            });
        }
        
        // Reject .. path segments (parent directory traversal)
        if arg.contains("..") {
            return Err(ToolError::ExecutionFailed {
                reason: format!("shell command argument contains '..' path segment (workspace containment violation): '{}'", arg),
            });
        }
        
        // Reject cd commands (only when it's the first arg, or explicit -c "cd ...")
        if arg == "cd" || arg.starts_with("cd ") || arg.contains("; cd ") || arg.contains("&& cd ") {
            return Err(ToolError::ExecutionFailed {
                reason: format!("shell command changes directory outside workspace: '{}'", arg),
            });
        }
        
        // Scan -c argument (bash -c, sh -c, zsh -c patterns)
        // If current arg is exactly "-c" and next arg exists, treat the next arg as a command string
        if arg == "-c" && i + 1 < args.len() {
            let shell_cmd = &args[i + 1];
            
            // Scan for absolute paths in the shell command string
            // Detect patterns like: > /path, >> /path, >& /path, etc.
            if shell_cmd.contains(">") && (shell_cmd.contains(">/") || shell_cmd.contains("> /") || shell_cmd.contains(">>/") || shell_cmd.contains(">> /")) {
                return Err(ToolError::ExecutionFailed {
                    reason: format!("shell -c command contains absolute path redirect (workspace containment violation): '{}'", shell_cmd),
                });
            }
            
            // Scan for .. in the shell command string
            if shell_cmd.contains("..") {
                return Err(ToolError::ExecutionFailed {
                    reason: format!("shell -c command contains '..' path segment (workspace containment violation): '{}'", shell_cmd),
                });
            }
            
            // Scan for cd commands in the shell command string
            if shell_cmd.contains("cd ") || shell_cmd.contains(";cd") || shell_cmd.contains("&&cd") || shell_cmd.contains("|cd") {
                return Err(ToolError::ExecutionFailed {
                    reason: format!("shell -c command changes directory outside workspace: '{}'", shell_cmd),
                });
            }
        }
    }
    Ok(())
}

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

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}: {}", code, stderr),
            });
        }

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }

    async fn call_streaming(
        &self,
        _args: Value,
        ctx: ToolContext,
    ) -> Result<Vec<ToolChunk>, ToolError> {
        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let mut child = Command::new("cargo")
            .arg("check")
            .current_dir(workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to spawn cargo check: {}", e),
            })?;

        let mut chunks = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) =
                lines
                    .next_line()
                    .await
                    .map_err(|e| ToolError::ExecutionFailed {
                        reason: format!("failed to read stdout: {}", e),
                    })?
            {
                chunks.push(ToolChunk {
                    delta: format!("{}\n", line),
                    is_final: false,
                });
            }
        }

        let exit_status = child.wait().await.map_err(|e| ToolError::ExecutionFailed {
            reason: format!("failed to wait for process: {}", e),
        })?;

        if !exit_status.success() {
            let code = exit_status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}", code),
            });
        }

        chunks.push(ToolChunk {
            delta: String::new(),
            is_final: true,
        });

        Ok(chunks)
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

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}: {}", code, stderr),
            });
        }

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }

    async fn call_streaming(
        &self,
        _args: Value,
        ctx: ToolContext,
    ) -> Result<Vec<ToolChunk>, ToolError> {
        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let mut child = Command::new("cargo")
            .arg("test")
            .current_dir(workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to spawn cargo test: {}", e),
            })?;

        let mut chunks = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) =
                lines
                    .next_line()
                    .await
                    .map_err(|e| ToolError::ExecutionFailed {
                        reason: format!("failed to read stdout: {}", e),
                    })?
            {
                chunks.push(ToolChunk {
                    delta: format!("{}\n", line),
                    is_final: false,
                });
            }
        }

        let exit_status = child.wait().await.map_err(|e| ToolError::ExecutionFailed {
            reason: format!("failed to wait for process: {}", e),
        })?;

        if !exit_status.success() {
            let code = exit_status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}", code),
            });
        }

        chunks.push(ToolChunk {
            delta: String::new(),
            is_final: true,
        });

        Ok(chunks)
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

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}: {}", code, stderr),
            });
        }

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

        // Validate command arguments for workspace containment
        validate_command_args_for_workspace_safety(&cmd_args)?;

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

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}: {}", code, stderr),
            });
        }

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }

    async fn call_streaming(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> Result<Vec<ToolChunk>, ToolError> {
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

        // Validate command arguments for workspace containment
        validate_command_args_for_workspace_safety(&cmd_args)?;

        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let mut cmd = Command::new(command);
        for arg in cmd_args {
            cmd.arg(arg);
        }
        cmd.current_dir(workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
            reason: format!("failed to spawn command: {}", e),
        })?;

        let mut chunks = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) =
                lines
                    .next_line()
                    .await
                    .map_err(|e| ToolError::ExecutionFailed {
                        reason: format!("failed to read stdout: {}", e),
                    })?
            {
                chunks.push(ToolChunk {
                    delta: format!("{}\n", line),
                    is_final: false,
                });
            }
        }

        let exit_status = child.wait().await.map_err(|e| ToolError::ExecutionFailed {
            reason: format!("failed to wait for process: {}", e),
        })?;

        if !exit_status.success() {
            let code = exit_status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}", code),
            });
        }

        chunks.push(ToolChunk {
            delta: String::new(),
            is_final: true,
        });

        Ok(chunks)
    }
}

/// Alias for shell_run_command with shorter name
pub struct ShellRunTool;

#[async_trait]
impl Tool for ShellRunTool {
    fn name(&self) -> &str {
        "shell.run"
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

        // Validate command arguments for workspace containment
        validate_command_args_for_workspace_safety(&cmd_args)?;

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

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}: {}", code, stderr),
            });
        }

        Ok(ToolOutput::text(format!("{}{}", stdout, stderr)))
    }

    async fn call_streaming(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> Result<Vec<ToolChunk>, ToolError> {
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

        // Validate command arguments for workspace containment
        validate_command_args_for_workspace_safety(&cmd_args)?;

        let workspace_root = &ctx.filesystem_policy.workspace_root;

        let mut cmd = Command::new(command);
        for arg in cmd_args {
            cmd.arg(arg);
        }
        cmd.current_dir(workspace_root)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| ToolError::ExecutionFailed {
            reason: format!("failed to spawn command: {}", e),
        })?;

        let mut chunks = Vec::new();

        if let Some(stdout) = child.stdout.take() {
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) =
                lines
                    .next_line()
                    .await
                    .map_err(|e| ToolError::ExecutionFailed {
                        reason: format!("failed to read stdout: {}", e),
                    })?
            {
                chunks.push(ToolChunk {
                    delta: format!("{}\n", line),
                    is_final: false,
                });
            }
        }

        let exit_status = child.wait().await.map_err(|e| ToolError::ExecutionFailed {
            reason: format!("failed to wait for process: {}", e),
        })?;

        if !exit_status.success() {
            let code = exit_status.code().unwrap_or(-1);
            return Err(ToolError::ExecutionFailed {
                reason: format!("Process exited with code {}", code),
            });
        }

        chunks.push(ToolChunk {
            delta: String::new(),
            is_final: true,
        });

        Ok(chunks)
    }
}

/// Factory function to create all shell tools
pub fn shell_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(CargoCheckTool),
        Arc::new(CargoTestTool),
        Arc::new(CargoFmtTool),
        Arc::new(RunCommandTool),
        Arc::new(ShellRunTool),
    ]
}
