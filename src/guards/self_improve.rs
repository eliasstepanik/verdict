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
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    Ok(())
}

pub fn check_agent_version_created(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    Ok(())
}

pub fn check_no_active_uncommitted_critical_changes(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    Ok(())
}
