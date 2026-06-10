#![allow(dead_code)]

//! Built-in filesystem tools

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;

use crate::tools::tool::{Tool, ToolOutput, ToolError, ToolSource, ToolContext};

/// Check if a path is within workspace root
fn is_within_workspace(path: &Path, workspace_root: &Path) -> bool {
    // Resolve the path; for not-yet-existing files canonicalize the parent
    // directory (which must exist) and re-attach the filename so we get the
    // same long-name form as canonicalize(workspace_root).
    let resolved = std::fs::canonicalize(path).or_else(|_| {
        match (path.parent(), path.file_name()) {
            (Some(parent), Some(name)) => {
                std::fs::canonicalize(parent).map(|p| p.join(name))
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no parent",
            )),
        }
    });

    let (Ok(canonical_path), Ok(canonical_root)) = (resolved, std::fs::canonicalize(workspace_root))
    else {
        return false;
    };

    // Strip the Windows \\?\ verbatim prefix before comparing so that a
    // canonicalized root and a fallback plain path compare equal.
    let norm_path = strip_verbatim_prefix_fs(&canonical_path);
    let norm_root = strip_verbatim_prefix_fs(&canonical_root);
    norm_path.starts_with(&norm_root)
}

/// Strip the Windows `\\?\` verbatim prefix so that plain and UNC-canonicalized
/// paths compare equal.  No-op on non-Windows platforms.
fn strip_verbatim_prefix_fs(path: &Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        std::path::PathBuf::from(&s[4..])
    } else {
        path.to_path_buf()
    }
}

/// Read file tool
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "fs.read"
    }

    fn description(&self) -> &str {
        "Read a file from the workspace"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to workspace root)"
                }
            },
            "required": ["path"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'path' field".to_string(),
            })?;

        let workspace_root = &ctx.filesystem_policy.workspace_root;
        let full_path = workspace_root.join(path_str);

        // Security check: ensure path is within workspace
        if !is_within_workspace(&full_path, workspace_root) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' escapes workspace root", path_str),
            });
        }

        // Check filesystem policy forbidden paths
        if !ctx.filesystem_policy.is_path_allowed(&full_path) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' is not allowed by filesystem policy (forbidden)", path_str),
            });
        }

        let content = fs::read_to_string(&full_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to read file: {}", e),
            })?;

        Ok(ToolOutput::text(content))
    }
}

/// Write file tool
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "fs.write"
    }

    fn description(&self) -> &str {
        "Write a file to the workspace (creates parent directories)"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to workspace root)"
                },
                "content": {
                    "type": "string",
                    "description": "File content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'path' field".to_string(),
            })?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'content' field".to_string(),
            })?;

        let workspace_root = &ctx.filesystem_policy.workspace_root;
        let full_path = workspace_root.join(path_str);

        // Security check: ensure path is within workspace
        if !is_within_workspace(&full_path, workspace_root) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' escapes workspace root", path_str),
            });
        }

        // Check filesystem policy forbidden paths
        if !ctx.filesystem_policy.is_path_allowed(&full_path) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' is not allowed by filesystem policy (forbidden)", path_str),
            });
        }

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    reason: format!("failed to create parent directories: {}", e),
                })?;
        }

        fs::write(&full_path, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to write file: {}", e),
            })?;

        Ok(ToolOutput::text(format!("File written: {}", path_str)))
    }
}

/// List directory tool
pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "fs.list"
    }

    fn description(&self) -> &str {
        "List contents of a directory"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path (relative to workspace root)"
                }
            },
            "required": ["path"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'path' field".to_string(),
            })?;

        let workspace_root = &ctx.filesystem_policy.workspace_root;
        let full_path = workspace_root.join(path_str);

        // Security check: ensure path is within workspace
        if !is_within_workspace(&full_path, workspace_root) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' escapes workspace root", path_str),
            });
        }

        let mut entries = fs::read_dir(&full_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to read directory: {}", e),
            })?;

        let mut entries_list = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to iterate directory: {}", e),
            })?
        {
            let path = entry.path();
            let name = entry
                .file_name()
                .to_string_lossy()
                .to_string();

            let entry_type = if path.is_dir() {
                "directory"
            } else {
                "file"
            };

            entries_list.push(json!({
                "name": name,
                "type": entry_type
            }));
        }

        Ok(ToolOutput::json(json!({
            "entries": entries_list
        })))
    }
}

/// Delete file tool
pub struct DeleteFileTool;

#[async_trait]
impl Tool for DeleteFileTool {
    fn name(&self) -> &str {
        "fs.delete"
    }

    fn description(&self) -> &str {
        "Delete a file from the workspace"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file (relative to workspace root)"
                }
            },
            "required": ["path"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'path' field".to_string(),
            })?;

        let workspace_root = &ctx.filesystem_policy.workspace_root;
        let full_path = workspace_root.join(path_str);

        // Security check: ensure path is within workspace
        if !is_within_workspace(&full_path, workspace_root) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' escapes workspace root", path_str),
            });
        }

        // Check filesystem policy forbidden paths
        if !ctx.filesystem_policy.is_path_allowed(&full_path) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' is not allowed by filesystem policy (forbidden)", path_str),
            });
        }

        fs::remove_file(&full_path)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("failed to delete file: {}", e),
            })?;

        Ok(ToolOutput::text(format!("File deleted: {}", path_str)))
    }
}

/// Factory function to create all filesystem tools
pub fn filesystem_tools() -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(ReadFileTool),
        Arc::new(WriteFileTool),
        Arc::new(ListDirTool),
        Arc::new(DeleteFileTool),
    ]
}
