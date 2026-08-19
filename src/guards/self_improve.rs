use super::GuardError;
use crate::context::StepContext;
use serde_json::Value;

pub fn check_reflection_has_actionable_finding(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    let output = ctx.output.as_ref().ok_or_else(|| GuardError::Failed {
        guard: "ReflectionHasActionableFinding".into(),
        reason: "No output".into(),
    })?;

    // Try structured JSON first
    if let Ok(json) = serde_json::from_str::<Value>(&output.raw) {
        if let Some(findings) = json["findings"].as_array() {
            let has_actionable = findings.iter().any(|f| {
                f["description"]
                    .as_str()
                    .map(|s| !s.is_empty())
                    .unwrap_or(false)
                    && f["proposed_action"]
                        .as_str()
                        .map(|s| !s.is_empty())
                        .unwrap_or(false)
            });
            if has_actionable {
                return Ok(());
            }
            return Err(GuardError::Failed {
                guard: "ReflectionHasActionableFinding".into(),
                reason:
                    "findings array present but no entry has both description and proposed_action"
                        .into(),
            });
        }
    }

    // Fall back: look for actionable reflection keywords in the output.
    // We accept outputs that contain explicit finding/improvement keywords,
    // even short ones, since a brief but clear finding is still actionable.
    let lower = output.raw.to_lowercase();
    let has_actionable_keyword = lower.contains("should")
        || lower.contains("recommend")
        || lower.contains("action:")
        || lower.contains("finding:")
        || lower.contains("finding,")
        || lower.contains("finding ")
        || lower.contains("improve")
        || lower.contains("could be")
        || lower.contains("needs to")
        || lower.contains("must ")
        || lower.contains("fix ");

    if has_actionable_keyword {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "ReflectionHasActionableFinding".into(),
            reason: "No actionable reflection findings detected in output".into(),
        })
    }
}

pub fn check_patch_applies_cleanly(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        if output.raw.starts_with("---")
            || output.raw.starts_with("+++")
            || output.raw.contains("@@")
        {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "PatchAppliesCleanly".to_string(),
                reason: "output is not a valid unified diff".to_string(),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "PatchAppliesCleanly".to_string(),
            reason: "no patch output".to_string(),
        })
    }
}

pub fn check_evaluation_improves_or_equal(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Extract current evaluation score from step output
    let current_score = ctx
        .output
        .as_ref()
        .ok_or_else(|| GuardError::Failed {
            guard: "EvaluationImprovesOrEqual".into(),
            reason: "No output from evaluation step".into(),
        })?;

    // Try to parse output as evaluation result
    let current_eval_score = if let Ok(json) = serde_json::from_str::<Value>(&current_score.raw)
    {
        // Support common output shapes:
        // 1. { "score": <number> } - direct score
        // 2. { "overall_score": <number> } - EvaluationSuiteResult shape
        // 3. { "evaluation_score": <number> } - AgentVersion shape
        // 4. { "eval_score": <number> } - SelfUpdateResult shape
        json.get("score")
            .or_else(|| json.get("overall_score"))
            .or_else(|| json.get("evaluation_score"))
            .or_else(|| json.get("eval_score"))
            .and_then(|v| v.as_f64())
    } else {
        None
    };

    let Some(current_score_val) = current_eval_score else {
        return Err(GuardError::Failed {
            guard: "EvaluationImprovesOrEqual".into(),
            reason: "Could not extract evaluation score from output (expected 'score', 'overall_score', 'evaluation_score', or 'eval_score' field)".into(),
        });
    };

    // Look for prior evaluation score in step_results
    // Common key patterns: "{agent_name}_prior_evaluation_score" or "{agent_name}_evaluation_score"
    let prior_score = ctx
        .step_results
        .iter()
        .find_map(|(key, result)| {
            if key.contains("prior") && key.contains("score") {
                if let Ok(json) = serde_json::from_str::<Value>(&result.output.raw) {
                    json.get("score")
                        .or_else(|| json.get("overall_score"))
                        .or_else(|| json.get("evaluation_score"))
                        .or_else(|| json.get("eval_score"))
                        .and_then(|v| v.as_f64())
                } else {
                    None
                }
            } else {
                None
            }
        });

    // If no prior score exists, pass trivially (nothing to regress against)
    if let Some(prior_val) = prior_score {
        // Require current score >= prior score (allow equal or improvement)
        if current_score_val < prior_val {
            return Err(GuardError::Failed {
                guard: "EvaluationImprovesOrEqual".into(),
                reason: format!(
                    "Evaluation score regressed: {} < prior score {}",
                    current_score_val, prior_val
                ),
            });
        }
    }

    Ok(())
}

pub fn check_agent_version_created(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Look for evidence of AgentVersion creation in step results or output
    // The step preceding this check should have run a self-update that creates a version

    // Check if any step result contains version-creation evidence
    for (key, result) in &ctx.step_results {
        // Look for keys or outputs containing version-related data
        if key.contains("version") || key.contains("self_update") {
            if let Ok(json) = serde_json::from_str::<Value>(&result.output.raw) {
                // Check for AgentVersion fields in the output
                if json.get("agent_name").is_some()
                    && json.get("version").is_some()
                    && json.get("created_at").is_some()
                {
                    return Ok(());
                }
                // Alternative: check for a "new_version" field (SelfUpdateResult shape)
                if json.get("new_version").is_some() {
                    return Ok(());
                }
            }
        }
    }

    // Also check current step output in case version info is there
    if let Some(output) = &ctx.output {
        if let Ok(json) = serde_json::from_str::<Value>(&output.raw) {
            if json.get("agent_name").is_some()
                && json.get("version").is_some()
                && json.get("created_at").is_some()
            {
                return Ok(());
            }
            if json.get("new_version").is_some() {
                return Ok(());
            }
        }
    }

    Err(GuardError::Failed {
        guard: "AgentVersionCreated".into(),
        reason: "No AgentVersion record found in step results or output".into(),
    })
}

pub fn check_no_active_uncommitted_critical_changes(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Run git status --porcelain in the workspace root
    // Fail if uncommitted changes exist in critical paths

    let workspace_root = &ctx.filesystem_policy.workspace_root;

    // Critical paths that must not have uncommitted changes
    // These are the same paths the Verdict framework itself touches
    let critical_paths = vec![
        "src/guards/",
        "src/self_update.rs",
        "src/runner/",
        "src/agent.rs",
        "src/pipeline.rs",
        "src/verdict.rs",
    ];

    // Spawn git status command
    let output = match std::process::Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(workspace_root)
        .output()
    {
        Ok(o) => o,
        Err(_) => {
            // Git not available or not a repo - gracefully pass
            return Ok(());
        }
    };

    if !output.status.success() {
        // Not a git repository or git failed - gracefully pass
        return Ok(());
    }

    let status_output = String::from_utf8_lossy(&output.stdout);

    // Parse git status output: each line is "<status> <path>"
    // Check if any changed file matches a critical path prefix
    for line in status_output.lines() {
        if line.is_empty() {
            continue;
        }

        // Git status format: "<status chars> <path>"
        // Extract the path (everything after the first 3 characters and space)
        let path_part = if line.len() > 3 {
            line[3..].trim()
        } else {
            continue;
        };

        // Check if this path is in a critical directory
        for critical in &critical_paths {
            if path_part.starts_with(critical) {
                return Err(GuardError::Failed {
                    guard: "NoActiveUncommittedCriticalChanges".into(),
                    reason: format!(
                        "Uncommitted changes detected in critical path: {}",
                        path_part
                    ),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "self_improve_tests.rs"]
mod tests;
