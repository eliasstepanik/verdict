use crate::context::StepContext;
use super::GuardError;

pub fn check_file_exists(
    _guard: &super::Guard,
    ctx: &StepContext,
    path: &str,
) -> Result<(), GuardError> {
    let full_path = ctx.filesystem_policy.workspace_root.join(path);
    if full_path.exists() {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "FileExists".to_string(),
            reason: format!("{} does not exist", path),
        })
    }
}

pub fn check_file_not_exists(
    _guard: &super::Guard,
    ctx: &StepContext,
    path: &str,
) -> Result<(), GuardError> {
    let full_path = ctx.filesystem_policy.workspace_root.join(path);
    if !full_path.exists() {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "FileNotExists".to_string(),
            reason: format!("{} exists", path),
        })
    }
}

pub fn check_file_contains(
    _guard: &super::Guard,
    ctx: &StepContext,
    path: &str,
    pattern: &str,
) -> Result<(), GuardError> {
    let full_path = ctx.filesystem_policy.workspace_root.join(path);
    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            if content.contains(pattern) {
                Ok(())
            } else {
                Err(GuardError::Failed {
                    guard: "FileContains".to_string(),
                    reason: format!("{} does not contain pattern", path),
                })
            }
        }
        Err(e) => Err(GuardError::IoError(e.to_string())),
    }
}

pub fn check_file_not_contains(
    _guard: &super::Guard,
    ctx: &StepContext,
    path: &str,
    pattern: &str,
) -> Result<(), GuardError> {
    let full_path = ctx.filesystem_policy.workspace_root.join(path);
    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            if !content.contains(pattern) {
                Ok(())
            } else {
                Err(GuardError::Failed {
                    guard: "FileNotContains".to_string(),
                    reason: format!("{} contains pattern", path),
                })
            }
        }
        Err(e) => Err(GuardError::IoError(e.to_string())),
    }
}

pub fn check_path_within_workspace(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw;
        // Check for common path traversal patterns
        if text.contains("../") || text.contains("..\\") {
            return Err(GuardError::Failed {
                guard: "PathWithinWorkspace".to_string(),
                reason: "output contains path traversal sequences".to_string(),
            });
        }
        // Check for absolute paths outside workspace
        let workspace_str = ctx.filesystem_policy.workspace_root.to_string_lossy();
        for line in text.lines() {
            if (line.contains('/') || line.contains('\\'))
                && !line.contains(workspace_str.as_ref())
                && (line.starts_with('/') || (line.len() > 2 && line.chars().nth(1) == Some(':')))
            {
                return Err(GuardError::Failed {
                    guard: "PathWithinWorkspace".to_string(),
                    reason: format!(
                        "output references absolute path outside workspace: {}",
                        line.trim()
                    ),
                });
            }
        }
    }
    Ok(())
}
