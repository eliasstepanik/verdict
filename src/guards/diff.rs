use super::GuardError;
use crate::context::StepContext;

pub fn check_max_diff_lines(
    _guard: &super::Guard,
    ctx: &StepContext,
    max_lines: usize,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let diff_lines = output
            .raw
            .lines()
            .filter(|l| l.starts_with('+') || l.starts_with('-'))
            .count();
        if diff_lines <= max_lines {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "MaxDiffLines".to_string(),
                reason: format!("diff has {} lines, max is {}", diff_lines, max_lines),
            })
        }
    } else {
        Ok(())
    }
}

pub fn check_max_changed_files(
    _guard: &super::Guard,
    ctx: &StepContext,
    max_files: usize,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let changed_files = output.raw.matches("diff --git").count();
        if changed_files <= max_files {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "MaxChangedFiles".to_string(),
                reason: format!("diff has {} files, max is {}", changed_files, max_files),
            })
        }
    } else {
        Ok(())
    }
}

pub fn check_diff_touches_allowed_paths(
    _guard: &super::Guard,
    ctx: &StepContext,
    allowed: &[String],
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        for line in output.raw.lines() {
            if line.starts_with("diff --git") {
                let mut matches = false;
                for allowed_path in allowed {
                    if line.contains(allowed_path) {
                        matches = true;
                        break;
                    }
                }
                if !matches {
                    return Err(GuardError::Failed {
                        guard: "DiffTouchesAllowedPaths".to_string(),
                        reason: "diff touches paths outside allowed list".to_string(),
                    });
                }
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}

pub fn check_diff_does_not_touch_forbidden_paths(
    _guard: &super::Guard,
    ctx: &StepContext,
    forbidden: &[String],
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        for line in output.raw.lines() {
            if line.starts_with("diff --git") {
                for forbidden_path in forbidden {
                    if line.contains(forbidden_path) {
                        return Err(GuardError::Failed {
                            guard: "DiffDoesNotTouchForbiddenPaths".to_string(),
                            reason: format!("diff touches forbidden path: {}", forbidden_path),
                        });
                    }
                }
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}
