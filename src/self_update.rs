//! Self-improvement flow — Phase 8
//! Full implementation of self-update system with patch proposal, validation, and application

use crate::agent::{Agent, AgentVersion};
use crate::injection::RiskLevel;
use chrono::Utc;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Configuration for self-update operations
#[derive(Debug, Clone)]
pub struct SelfUpdateConfig {
    /// Paths the patch may touch (whitelist)
    pub allowed_paths: Vec<String>,

    /// Paths the patch must NOT touch (blacklist)
    pub forbidden_paths: Vec<String>,

    /// Whether user approval is required
    pub require_approval: bool,

    /// Sandbox directory for applying patches
    pub sandbox_dir: Option<PathBuf>,

    /// Whether to run evaluation suite after applying
    pub run_eval_after: bool,
}

impl Default for SelfUpdateConfig {
    fn default() -> Self {
        Self {
            allowed_paths: vec!["src/agents/".to_string(), "src/skills/".to_string()],
            forbidden_paths: vec![
                "src/runner.rs".to_string(),
                "src/guard.rs".to_string(),
                "src/verdict.rs".to_string(),
                "Cargo.toml".to_string(),
            ],
            require_approval: true,
            sandbox_dir: None,
            run_eval_after: true,
        }
    }
}

/// A proposed self-update for an agent
#[derive(Debug, Clone)]
pub struct SelfUpdateProposal {
    /// Unified diff format patch
    pub patch: String,

    /// Summary of changes
    pub summary: String,

    /// Risk assessment
    pub risk_level: RiskLevel,
}

/// Result of a self-update operation
#[derive(Debug, Clone)]
pub struct SelfUpdateResult {
    /// Whether the patch was successfully applied
    pub applied: bool,

    /// New agent version if applied
    pub new_version: Option<AgentVersion>,

    /// Reason for rejection/failure if not applied
    pub reason: Option<String>,

    /// Evaluation score after applying (if run_eval_after was true)
    pub eval_score: Option<f64>,
}

/// Error type for self-update operations
#[derive(Error, Debug)]
pub enum SelfUpdateError {
    #[error("patch is not a valid unified diff")]
    InvalidDiff,

    #[error("patch touches forbidden path: {path}")]
    ForbiddenPath { path: String },

    #[error("patch is empty")]
    EmptyPatch,

    #[error("patch application failed: {0}")]
    PatchApplyFailed(String),

    #[error("compile validation failed: {reason}")]
    CompileFailed { reason: String },

    #[error("test validation failed: {reason}")]
    TestFailed { reason: String },

    #[error("sandbox setup failed: {0}")]
    SandboxSetupFailed(String),
    #[error("I/O error: {0}")]
    Io(String),
}

/// Recursively copy a directory from src to dst
async fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), SelfUpdateError> {
    // Create destination directory
    tokio::fs::create_dir_all(dst)
        .await
        .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

    let mut entries = tokio::fs::read_dir(src)
        .await
        .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| SelfUpdateError::Io(e.to_string()))?
    {
        let path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if path.is_dir() {
            // Recursively copy subdirectory
            Box::pin(copy_dir_recursive(&path, &dst_path)).await?;
        } else {
            // Copy file
            tokio::fs::copy(&path, &dst_path)
                .await
                .map_err(|e| SelfUpdateError::Io(e.to_string()))?;
        }
    }

    Ok(())
}
/// Normalize a path lexically by resolving `.` and `..` components
fn normalize_path_lexical(path: &str) -> String {
    let mut components: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            ".." => {
                components.pop();
            }
            "." | "" => {}
            p => components.push(p),
        }
    }
    components.join("/")
}

/// Extract file paths being modified from a unified diff
fn extract_modified_paths(patch: &str) -> Vec<String> {
    patch
        .lines()
        .filter_map(|line| {
            // Match "+++ b/path" and "--- a/path" diff headers
            if let Some(rest) = line.strip_prefix("+++ b/") {
                Some(rest.trim().to_string())
            } else if let Some(rest) = line.strip_prefix("--- a/") {
                // Skip /dev/null
                if rest.trim() == "/dev/null" {
                    None
                } else {
                    Some(rest.trim().to_string())
                }
            } else {
                None
            }
        })
        .collect()
}

/// Engine for managing self-updates
pub struct SelfUpdateEngine;

impl SelfUpdateEngine {
    /// Validate a patch proposal against static checks
    pub fn validate_proposal(
        proposal: &SelfUpdateProposal,
        config: &SelfUpdateConfig,
    ) -> Result<(), SelfUpdateError> {
        // Check if patch is empty
        if proposal.patch.trim().is_empty() {
            return Err(SelfUpdateError::EmptyPatch);
        }

        // Check if patch is a valid unified diff (has diff markers)
        if !proposal.patch.contains("--- ")
            && !proposal.patch.contains("+++ ")
            && !proposal.patch.contains("@@")
        {
            return Err(SelfUpdateError::InvalidDiff);
        }

        // Extract actual file paths from diff headers
        let modified_paths = extract_modified_paths(&proposal.patch);

        // Fail-closed: if patch is non-empty but no paths found, reject it
        if modified_paths.is_empty() && !proposal.patch.trim().is_empty() {
            return Err(SelfUpdateError::InvalidDiff);
        }

        // Check that all modified paths are within allowed_paths
        for modified in &modified_paths {
            let normalized = normalize_path_lexical(modified);
            let mut found_allowed = false;
            for allowed in &config.allowed_paths {
                if normalized.starts_with(allowed) || normalized == *allowed {
                    found_allowed = true;
                    break;
                }
            }
            if !found_allowed {
                return Err(SelfUpdateError::ForbiddenPath {
                    path: modified.clone(),
                });
            }
        }

        // Check for forbidden paths in the patch
        for modified in &modified_paths {
            let normalized = normalize_path_lexical(modified);
            for forbidden_path in &config.forbidden_paths {
                if normalized.starts_with(forbidden_path) || normalized == *forbidden_path {
                    return Err(SelfUpdateError::ForbiddenPath {
                        path: modified.clone(),
                    });
                }
            }
        }

        // Risk-based validation
        match proposal.risk_level {
            RiskLevel::Critical => {
                // Critical risk: require the patch to be small (max 50 changed lines)
                let changed_lines = proposal
                    .patch
                    .lines()
                    .filter(|l| {
                        (l.starts_with('+') && !l.starts_with("+++"))
                            || (l.starts_with('-') && !l.starts_with("---"))
                    })
                    .count();
                if changed_lines > 50 {
                    return Err(SelfUpdateError::InvalidDiff);
                }
            }
            RiskLevel::High => {
                // High risk: ensure patch summary is non-empty
                if proposal.summary.trim().is_empty() {
                    return Err(SelfUpdateError::EmptyPatch);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Apply a patch in a sandbox directory with validation
    pub async fn apply_in_sandbox(
        patch: &str,
        sandbox_dir: &Path,
        workspace_root: &Path,
    ) -> Result<(), SelfUpdateError> {
        // Step 1: Copy workspace_root contents to sandbox_dir
        copy_dir_recursive(workspace_root, sandbox_dir)
            .await
            .map_err(|e| SelfUpdateError::SandboxSetupFailed(e.to_string()))?;

        // Step 2: Write the patch file
        let patch_path = sandbox_dir.join("__verdict_patch__.diff");
        tokio::fs::write(&patch_path, patch)
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

        // Step 3: Validate patch is a unified diff
        if !patch.contains("--- ") && !patch.contains("+++ ") && !patch.contains("@@") {
            return Err(SelfUpdateError::InvalidDiff);
        }

        // Step 4: git apply --check (dry run)
        let check = tokio::process::Command::new("git")
            .args(["apply", "--check", patch_path.to_str().unwrap()])
            .current_dir(sandbox_dir)
            .output()
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

        if !check.status.success() {
            return Err(SelfUpdateError::PatchApplyFailed(
                String::from_utf8_lossy(&check.stderr).into_owned(),
            ));
        }

        // Step 5: git apply (actual apply)
        let apply = tokio::process::Command::new("git")
            .args(["apply", patch_path.to_str().unwrap()])
            .current_dir(sandbox_dir)
            .output()
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

        if !apply.status.success() {
            return Err(SelfUpdateError::PatchApplyFailed(
                String::from_utf8_lossy(&apply.stderr).into_owned(),
            ));
        }

        Ok(())
    }

    /// Create a new AgentVersion from the current agent and a change summary
    pub fn version_agent(
        agent: &Agent,
        change_summary: &str,
        eval_score: Option<f64>,
    ) -> AgentVersion {
        // No parent version for a freshly created agent version
        let parent_version = None;

        // Try to get current git HEAD commit hash
        let git_commit = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
                } else {
                    None
                }
            });

        AgentVersion {
            agent_name: agent.name.clone(),
            version: chrono::Utc::now().format("%Y%m%d%H%M%S").to_string(),
            parent_version,
            created_at: Utc::now(),
            change_summary: change_summary.to_string(),
            git_commit,
            evaluation_score: eval_score,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_proposal_empty_patch() {
        let proposal = SelfUpdateProposal {
            patch: "".to_string(),
            summary: "test".to_string(),
            risk_level: RiskLevel::Low,
        };
        let config = SelfUpdateConfig::default();

        let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
        assert!(result.is_err());
        match result {
            Err(SelfUpdateError::EmptyPatch) => (),
            _ => panic!("expected EmptyPatch error"),
        }
    }

    #[test]
    fn test_validate_proposal_not_a_diff() {
        let proposal = SelfUpdateProposal {
            patch: "this is not a diff".to_string(),
            summary: "test".to_string(),
            risk_level: RiskLevel::Low,
        };
        let config = SelfUpdateConfig::default();

        let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
        assert!(result.is_err());
        match result {
            Err(SelfUpdateError::InvalidDiff) => (),
            _ => panic!("expected InvalidDiff error"),
        }
    }

    #[test]
    fn test_validate_proposal_forbidden_path() {
        let proposal = SelfUpdateProposal {
            patch: "--- a/Cargo.toml\n+++ b/Cargo.toml\n@@ -1,1 +1,1 @@".to_string(),
            summary: "test".to_string(),
            risk_level: RiskLevel::Low,
        };
        let config = SelfUpdateConfig::default();

        let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
        assert!(result.is_err());
        match result {
            Err(SelfUpdateError::ForbiddenPath { .. }) => (),
            _ => panic!("expected ForbiddenPath error"),
        }
    }

    #[test]
    fn test_validate_proposal_valid_diff() {
        let proposal = SelfUpdateProposal {
            patch: "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,2 @@\n content"
                .to_string(),
            summary: "test".to_string(),
            risk_level: RiskLevel::Low,
        };
        let config = SelfUpdateConfig::default();

        let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
        assert!(result.is_ok());
    }
}
