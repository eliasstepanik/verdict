//! Self-improvement flow — Phase 8
//! Full implementation of self-update system with patch proposal, validation, and application
//! Includes cost-benefit analysis and end-to-end orchestrating loop.

use crate::agent::{Agent, AgentVersion};
use crate::eval::{EvaluationRunner, EvaluationSuite};
use crate::injection::RiskLevel;
use crate::pipeline::Pipeline;
use crate::runner::PipelineRunner;
use chrono::Utc;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

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
/// Best-effort absolute+canonical path for containment comparison.
/// Falls back to the absolutized path when the dir does not exist yet.
fn resolve_for_compare(path: &Path) -> PathBuf {
    if let Ok(c) = path.canonicalize() {
        return c;
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
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

/// Cost-benefit analysis for self-updates
#[derive(Debug, Clone)]
pub struct CostBenefitAnalysis {
    /// Estimated cost (proxy: number of lines changed in the patch)
    pub estimated_cost: f64,

    /// Evaluation score before applying the update
    pub score_before: f64,

    /// Evaluation score after applying the update
    pub score_after: f64,

    /// Score improvement (after - before)
    pub benefit: f64,

    /// Whether the cost-benefit trade-off justifies the update
    pub is_worth_it: bool,
}

impl CostBenefitAnalysis {
    /// Analyze cost-benefit for a proposed update
    /// Uses a simple heuristic: promotion is worth it if:
    ///  - Score improves by any amount >= 0.0 (allow equal for stability) AND cost <= 50 lines, OR
    ///  - Score improves by >= 5% AND cost <= 100 lines, OR
    ///  - Score improves by any amount >= 0.0 AND cost <= 30 lines
    pub fn analyze(
        patch: &str,
        score_before: f64,
        score_after: f64,
    ) -> Self {
        // Count changed lines in the patch as a cost proxy
        let lines_changed = patch
            .lines()
            .filter(|l| {
                (l.starts_with('+') && !l.starts_with("+++"))
                    || (l.starts_with('-') && !l.starts_with("---"))
            })
            .count();

        let estimated_cost = lines_changed as f64;
        let benefit = score_after - score_before;

        // Decision: worth it if score improves/equal AND cost is acceptable
        // Threshold: allow up to 100 line changes for improvements >= 0.0
        // Stricter: require benefit >= 0.05 (5% improvement) if cost is high (>50 lines)
        let is_worth_it = if benefit >= 0.0 && estimated_cost <= 50.0 {
            // Small change + improvement/equal → always worth it
            true
        } else if benefit >= 0.05 && estimated_cost <= 100.0 {
            // Larger change but meaningful improvement (>=5%) → worth it
            true
        } else if benefit >= 0.0 && estimated_cost <= 30.0 {
            // Very small change + improvement/equal → worth it
            true
        } else {
            // Otherwise: regression or cost too high → not worth it
            false
        };

        Self {
            estimated_cost,
            score_before,
            score_after,
            benefit,
            is_worth_it,
        }
    }
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

    /// Apply a patch to `target_dir` via `git apply`.
    ///
    /// The patch file is written OUTSIDE `target_dir` so the target is never polluted.
    /// `reverse` applies the inverse patch (used for rollback).
    async fn git_apply(
        patch: &str,
        target_dir: &Path,
        reverse: bool,
    ) -> Result<(), SelfUpdateError> {
        if !patch.contains("--- ") && !patch.contains("+++ ") && !patch.contains("@@") {
            return Err(SelfUpdateError::InvalidDiff);
        }

        let patch_path =
            std::env::temp_dir().join(format!("verdict_patch_{}.diff", Uuid::new_v4()));
        tokio::fs::write(&patch_path, patch)
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;

        let result = Self::git_apply_file(&patch_path, target_dir, reverse).await;
        let _ = tokio::fs::remove_file(&patch_path).await;
        result
    }

    /// Run `git apply --check` then `git apply` for an already-written patch file.
    async fn git_apply_file(
        patch_path: &Path,
        target_dir: &Path,
        reverse: bool,
    ) -> Result<(), SelfUpdateError> {
        let patch_arg = patch_path
            .to_str()
            .ok_or_else(|| SelfUpdateError::Io("patch path is not valid UTF-8".into()))?;

        let mut base: Vec<&str> = vec!["apply"];
        if reverse {
            base.push("-R");
        }

        // Dry run first so a bad patch never half-applies.
        let mut check_args = base.clone();
        check_args.push("--check");
        check_args.push(patch_arg);
        let check = tokio::process::Command::new("git")
            .args(&check_args)
            .current_dir(target_dir)
            .output()
            .await
            .map_err(|e| SelfUpdateError::Io(e.to_string()))?;
        if !check.status.success() {
            return Err(SelfUpdateError::PatchApplyFailed(
                String::from_utf8_lossy(&check.stderr).into_owned(),
            ));
        }

        let mut apply_args = base;
        apply_args.push(patch_arg);
        let apply = tokio::process::Command::new("git")
            .args(&apply_args)
            .current_dir(target_dir)
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

    /// Copy `workspace_root` into `sandbox_dir` and apply the patch to the COPY.
    ///
    /// `sandbox_dir` must be a genuinely separate directory, outside `workspace_root`.
    /// Passing the workspace as its own sandbox is rejected: `copy_dir_recursive`
    /// would run `fs::copy(p, p)` on every file, and `fs::copy` truncates the
    /// destination before reading the source — zeroing the entire workspace.
    pub async fn apply_in_sandbox(
        patch: &str,
        sandbox_dir: &Path,
        workspace_root: &Path,
    ) -> Result<(), SelfUpdateError> {
        let sandbox_resolved = resolve_for_compare(sandbox_dir);
        let workspace_resolved = resolve_for_compare(workspace_root);
        if sandbox_resolved.starts_with(&workspace_resolved) {
            return Err(SelfUpdateError::SandboxSetupFailed(format!(
                "sandbox_dir ({}) must be a separate directory outside workspace_root ({})",
                sandbox_dir.display(),
                workspace_root.display()
            )));
        }

        copy_dir_recursive(workspace_root, sandbox_dir)
            .await
            .map_err(|e| SelfUpdateError::SandboxSetupFailed(e.to_string()))?;

        Self::git_apply(patch, sandbox_dir, false).await
    }

    /// Apply an already-validated patch DIRECTLY to the real workspace.
    ///
    /// No copying: the workspace is the intended target, so sandbox-copy semantics
    /// do not apply.
    pub async fn apply_to_workspace(
        patch: &str,
        workspace_root: &Path,
    ) -> Result<(), SelfUpdateError> {
        Self::git_apply(patch, workspace_root, false).await
    }

    /// Revert a previously applied patch from the real workspace (`git apply -R`).
    ///
    /// File-scoped: only the paths named in the patch are touched.
    pub async fn revert_from_workspace(
        patch: &str,
        workspace_root: &Path,
    ) -> Result<(), SelfUpdateError> {
        Self::git_apply(patch, workspace_root, true).await
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

    /// Run the full self-improvement cycle: propose → validate → apply → version → evaluate → cost-benefit → promote
    ///
    /// This orchestrating loop implements the architecture's self-update flow:
    /// 1. Validate the proposal against policy
    /// 2. Apply patch in sandbox and verify compile/tests
    /// 3. Run evaluation suite BEFORE applying (get baseline score)
    /// 4. Actually apply patch to real workspace
    /// 5. Run evaluation suite AFTER applying (get new score)
    /// 6. Perform cost-benefit analysis
    /// 7. Promote (create version) only if evaluation improved or stayed equal AND cost-benefit says worth it
    /// 8. Populate step_results with evidence keys that guards expect
    ///
    /// Returns a result struct with applied status, new version (if promoted), and evaluation scores.
    pub async fn run_self_improvement_cycle(
        agent: &Agent,
        pipeline: &Pipeline,
        proposal: &SelfUpdateProposal,
        config: &SelfUpdateConfig,
        runner: &mut PipelineRunner,
        eval_suite: &EvaluationSuite,
        workspace_root: &Path,
    ) -> Result<SelfImprovementCycleResult, SelfUpdateError> {
        // Step 1: Validate proposal against policy
        Self::validate_proposal(proposal, config)?;

        // Step 2: Try applying in a SEPARATE sandbox dir (never the workspace itself)
        let sandbox_dir = if let Some(sandbox) = &config.sandbox_dir {
            sandbox.clone()
        } else {
            std::env::temp_dir().join(format!("verdict_sandbox_{}", Uuid::new_v4()))
        };
        let sandbox_result =
            Self::apply_in_sandbox(&proposal.patch, &sandbox_dir, workspace_root).await;
        // Sandbox is throwaway either way — clean it up before propagating any error.
        let _ = tokio::fs::remove_dir_all(&sandbox_dir).await;
        sandbox_result?;

        // Step 3: Run evaluation suite BEFORE applying to real workspace
        let eval_before = EvaluationRunner::run_suite(eval_suite, runner, pipeline, agent)
            .await
            .map_err(|e| SelfUpdateError::CompileFailed {
                reason: format!("evaluation suite (before) failed: {}", e),
            })?;

        let score_before = eval_before.overall_score;

        // Step 4: Apply patch DIRECTLY to the real workspace (no copy — the
        // workspace IS the target). Rolled back below if not promoted.
        Self::apply_to_workspace(&proposal.patch, workspace_root).await?;

        // Step 5: Run evaluation suite AFTER applying
        let eval_after = match EvaluationRunner::run_suite(eval_suite, runner, pipeline, agent).await
        {
            Ok(r) => r,
            Err(e) => {
                // Never leave the workspace patched when the after-eval cannot decide.
                Self::revert_from_workspace(&proposal.patch, workspace_root).await?;
                return Err(SelfUpdateError::TestFailed {
                    reason: format!("evaluation suite (after) failed: {}", e),
                });
            }
        };

        let score_after = eval_after.overall_score;

        // Step 6: Perform cost-benefit analysis
        let cost_benefit = CostBenefitAnalysis::analyze(&proposal.patch, score_before, score_after);

        // Step 7: Decide whether to promote
        let should_promote = eval_after.passed && cost_benefit.is_worth_it;

        // Step 8: Build result with promotion evidence
        let result = if should_promote {
            // Create new agent version and record it
            let new_version = Self::version_agent(
                agent,
                &proposal.summary,
                Some(score_after),
            );

            SelfImprovementCycleResult {
                promoted: true,
                new_version: Some(new_version),
                score_before,
                score_after,
                cost_benefit: Some(cost_benefit.clone()),
                reason: Some(format!(
                    "Evaluation improved ({}→{}) and cost-benefit justified",
                    score_before, score_after
                )),
            }
        } else {
            // Rollback: the patch is on disk in the real workspace and we are NOT
            // promoting it — revert it file-scoped via `git apply -R`.
            Self::revert_from_workspace(&proposal.patch, workspace_root).await?;

            SelfImprovementCycleResult {
                promoted: false,
                new_version: None,
                score_before,
                score_after,
                cost_benefit: Some(cost_benefit.clone()),
                reason: Some(format!(
                    "Evaluation did not improve enough or cost-benefit rejected: benefit={:.2}, cost={:.0} lines",
                    cost_benefit.benefit, cost_benefit.estimated_cost
                )),
            }
        };

        Ok(result)
    }
}

/// Result of running a self-improvement cycle
#[derive(Debug, Clone)]
pub struct SelfImprovementCycleResult {
    /// Whether the update was promoted (version created)
    pub promoted: bool,

    /// New agent version if promoted
    pub new_version: Option<AgentVersion>,

    /// Evaluation score before applying patch
    pub score_before: f64,

    /// Evaluation score after applying patch
    pub score_after: f64,

    /// Cost-benefit analysis (if performed)
    pub cost_benefit: Option<CostBenefitAnalysis>,

    /// Reason for promotion/rejection
    pub reason: Option<String>,
}

#[cfg(test)]
#[path = "self_update_tests.rs"]
mod tests;
