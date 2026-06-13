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
            allowed_paths: vec!["src/agents/".to_string(), "skills/".to_string()],
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

    #[error("patch application failed: {reason}")]
    PatchFailed { reason: String },

    #[error("patch apply failed: {0}")]
    PatchApplyFailed(String),

    #[error("compile validation failed: {reason}")]
    CompileFailed { reason: String },

    #[error("test validation failed: {reason}")]
    TestFailed { reason: String },

    #[error("I/O error: {0}")]
    Io(String),
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
        if !proposal.patch.contains("--- ") && !proposal.patch.contains("+++ ")
            && !proposal.patch.contains("@@")
        {
            return Err(SelfUpdateError::InvalidDiff);
        }

        // Check for forbidden paths in the patch
        for forbidden_path in &config.forbidden_paths {
            if proposal.patch.contains(forbidden_path) {
                return Err(SelfUpdateError::ForbiddenPath {
                    path: forbidden_path.clone(),
                });
            }
        }

        Ok(())
    }

    /// Apply a patch in a sandbox directory with validation
    pub async fn apply_in_sandbox(
        patch: &str,
        sandbox_dir: &Path,
        _workspace_root: &Path,
    ) -> Result<(), SelfUpdateError> {
        // Ensure sandbox dir exists
        if !sandbox_dir.exists() {
            std::fs::create_dir_all(sandbox_dir)
                .map_err(|e| SelfUpdateError::Io(e.to_string()))?;
        }

        // Validate patch is a unified diff
        if !patch.contains("--- ") && !patch.contains("+++ ") && !patch.contains("@@") {
            return Err(SelfUpdateError::InvalidDiff);
        }

        // Write the patch to a file in the sandbox
        let patch_path = sandbox_dir.join("patch.diff");
        tokio::fs::write(&patch_path, patch)
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

        // Step 1: git apply --check (dry run)
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

        // Step 2: git apply
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

        // Step 3: cargo check — verify the patched code still compiles.
        let check_build = tokio::process::Command::new("cargo")
            .args(["check", "--quiet"])
            .current_dir(sandbox_dir)
            .output()
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

        if !check_build.status.success() {
            return Err(SelfUpdateError::CompileFailed {
                reason: String::from_utf8_lossy(&check_build.stderr).into_owned(),
            });
        }

        // Step 4: cargo test — verify the patched code still passes tests.
        let test_run = tokio::process::Command::new("cargo")
            .args(["test", "--quiet", "--no-fail-fast"])
            .current_dir(sandbox_dir)
            .output()
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

        if !test_run.status.success() {
            return Err(SelfUpdateError::TestFailed {
                reason: String::from_utf8_lossy(&test_run.stderr).into_owned(),
            });
        }

        Ok(())
    }

    /// Create a new AgentVersion from the current agent and a change summary
    pub fn version_agent(
        agent: &Agent,
        change_summary: &str,
        eval_score: Option<f64>,
    ) -> AgentVersion {
        AgentVersion {
            agent_name: agent.name.clone(),
            version: chrono::Utc::now()
                .format("%Y%m%d%H%M%S")
                .to_string(),
            parent_version: None,
            created_at: Utc::now(),
            change_summary: change_summary.to_string(),
            git_commit: None,
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
            patch: "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,2 @@\n content".to_string(),
            summary: "test".to_string(),
            risk_level: RiskLevel::Low,
        };
        let config = SelfUpdateConfig::default();

        let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
        assert!(result.is_ok());
    }
}
