use super::GuardError;
use crate::context::StepContext;

pub fn check_no_forbidden_tools(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    Ok(())
}

pub fn check_only_allowed_tools(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    Ok(())
}

pub fn check_shell_allowlist(
    _guard: &super::Guard,
    ctx: &StepContext,
    cmds: &[String],
) -> Result<(), GuardError> {
    let output = match &ctx.output {
        Some(o) => &o.raw,
        None => return Ok(()),
    };

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let allowed = cmds
            .iter()
            .any(|prefix| trimmed.starts_with(prefix.as_str()));
        if !allowed {
            return Err(GuardError::Failed {
                guard: "ShellCommandAllowlist".to_string(),
                reason: format!("command '{}' not in allowlist", trimmed),
            });
        }
    }
    Ok(())
}

pub fn check_shell_denylist(
    _guard: &super::Guard,
    _ctx: &StepContext,
    _cmds: &[String],
) -> Result<(), GuardError> {
    Ok(())
}
