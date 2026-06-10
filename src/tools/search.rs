#![allow(dead_code)]

//! Built-in search tools

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

use crate::tools::tool::{Tool, ToolOutput, ToolError, ToolSource, ToolContext};

/// Search files by glob pattern
pub struct SearchFilesTool;

#[async_trait]
impl Tool for SearchFilesTool {
    fn name(&self) -> &str {
        "search.files"
    }

    fn description(&self) -> &str {
        "Search for files matching a pattern in a directory"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "File name pattern to match (simple substring match)"
                },
                "root": {
                    "type": "string",
                    "description": "Root directory to search (relative to workspace root)"
                }
            },
            "required": ["pattern", "root"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'pattern' field".to_string(),
            })?;

        let root_str = args
            .get("root")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'root' field".to_string(),
            })?;

        let workspace_root = &ctx.filesystem_policy.workspace_root;
        let search_root = workspace_root.join(root_str);

        // Use iterative search via std (not async-recursive)
        let mut matches = Vec::new();
        search_files_iterative(&search_root, pattern, &mut matches);

        Ok(ToolOutput::json(json!({
            "matches": matches,
            "count": matches.len()
        })))
    }
}

fn search_files_iterative(path: &Path, pattern: &str, matches: &mut Vec<String>) {
    if !path.exists() {
        return;
    }

    let mut dirs_to_process = vec![path.to_path_buf()];

    while let Some(current_dir) = dirs_to_process.pop() {
        if let Ok(entries) = std::fs::read_dir(&current_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let entry_path = entry.path();
                    let file_name = entry
                        .file_name()
                        .to_string_lossy()
                        .to_string();

                    if file_name.contains(pattern) {
                        matches.push(entry_path.to_string_lossy().to_string());
                    }

                    if entry_path.is_dir() {
                        dirs_to_process.push(entry_path);
                    }
                }
            }
        }
    }
}

/// Search within files for a pattern
pub struct GrepTool;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "search.grep"
    }

    fn description(&self) -> &str {
        "Search for a pattern within files (simple string matching)"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "File or directory path to search in"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "Search recursively in directories (default: true)"
                }
            },
            "required": ["pattern", "path"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let pattern = args
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'pattern' field".to_string(),
            })?;

        let path_str = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing 'path' field".to_string(),
            })?;

        let recursive = args
            .get("recursive")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let workspace_root = &ctx.filesystem_policy.workspace_root;
        let search_path = workspace_root.join(path_str);

        let mut matches = Vec::new();

        if search_path.is_file() {
            grep_file_sync(&search_path, pattern, 1, &mut matches);
        } else if search_path.is_dir() && recursive {
            grep_directory_recursive_sync(&search_path, pattern, &mut matches);
        } else if search_path.is_dir() {
            grep_directory_sync(&search_path, pattern, &mut matches);
        }

        Ok(ToolOutput::json(json!({
            "matches": matches,
            "count": matches.len()
        })))
    }
}

fn grep_file_sync(path: &Path, pattern: &str, line_offset: usize, matches: &mut Vec<Value>) {
    if let Ok(content) = std::fs::read_to_string(path) {
        for (idx, line) in content.lines().enumerate() {
            if line.contains(pattern) {
                matches.push(json!({
                    "file": path.to_string_lossy().to_string(),
                    "line": idx + line_offset,
                    "content": line
                }));
            }
        }
    }
}

fn grep_directory_sync(path: &Path, pattern: &str, matches: &mut Vec<Value>) {
    if !path.exists() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries {
            if let Ok(entry) = entry {
                let entry_path = entry.path();
                if entry_path.is_file() {
                    grep_file_sync(&entry_path, pattern, 1, matches);
                }
            }
        }
    }
}

fn grep_directory_recursive_sync(path: &Path, pattern: &str, matches: &mut Vec<Value>) {
    if !path.exists() {
        return;
    }

    let mut dirs_to_process = vec![path.to_path_buf()];

    while let Some(current_dir) = dirs_to_process.pop() {
        if let Ok(entries) = std::fs::read_dir(&current_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let entry_path = entry.path();
                    if entry_path.is_file() {
                        grep_file_sync(&entry_path, pattern, 1, matches);
                    } else if entry_path.is_dir() {
                        dirs_to_process.push(entry_path);
                    }
                }
            }
        }
    }
}

/// Factory function to create all search tools
pub fn search_tools() -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(SearchFilesTool), Arc::new(GrepTool)]
}
