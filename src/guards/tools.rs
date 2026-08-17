use super::GuardError;
use crate::context::StepContext;
use crate::toolset::ToolSet;

pub fn check_no_forbidden_tools(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Check if the allowed_tools is a Deny set with forbidden items
    if let ToolSet::Deny(forbidden) = &ctx.allowed_tools {
        for tool_name in &ctx.tools_used {
            if forbidden.iter().any(|f| f == tool_name) {
                return Err(GuardError::Failed {
                    guard: "NoForbiddenToolsUsed".to_string(),
                    reason: format!("forbidden tool '{}' was used", tool_name),
                });
            }
        }
    }
    Ok(())
}

pub fn check_only_allowed_tools(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Use contains_with_skill_registry to support FromSkill tool variants
    for tool_name in &ctx.tools_used {
        if !ctx.allowed_tools.contains_with_skill_registry(tool_name, &ctx.skill_registry) {
            return Err(GuardError::Failed {
                guard: "OnlyAllowedToolsUsed".to_string(),
                reason: format!("tool '{}' is not in allowed tools", tool_name),
            });
        }
    }
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
        let denied = cmds
            .iter()
            .any(|substr| trimmed.contains(substr.as_str()));
        if denied {
            return Err(GuardError::Failed {
                guard: "ShellCommandDenylist".to_string(),
                reason: format!("denied command/substring found in: '{}'", trimmed),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::StepOutput;
    use crate::agent::FilesystemPolicy;
    use crate::context::StepContext;
    use crate::toolset::ToolSet;

    fn make_test_context() -> StepContext {
        let mut ctx = StepContext::new(
            "test_agent".to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            serde_json::json!({}),
            FilesystemPolicy::default(),
        );
        ctx.allowed_tools = ToolSet::Full;
        ctx
    }

    // ============= check_no_forbidden_tools tests =============

    #[test]
    fn test_no_forbidden_tools_not_in_denylist() {
        let mut ctx = make_test_context();
        ctx.allowed_tools = ToolSet::Deny(vec!["dangerous_tool".to_string(), "risky_tool".to_string()]);
        ctx.tools_used = vec!["safe_tool".to_string()];

        let guard = super::super::Guard::NoForbiddenToolsUsed;
        let result = check_no_forbidden_tools(&guard, &ctx);
        assert!(
            result.is_ok(),
            "Should pass when tool is not in forbidden list"
        );
    }

    #[test]
    fn test_no_forbidden_tools_in_denylist() {
        let mut ctx = make_test_context();
        ctx.allowed_tools = ToolSet::Deny(vec!["dangerous_tool".to_string(), "risky_tool".to_string()]);
        ctx.tools_used = vec!["dangerous_tool".to_string()];

        let guard = super::super::Guard::NoForbiddenToolsUsed;
        let result = check_no_forbidden_tools(&guard, &ctx);
        assert!(
            result.is_err(),
            "Should fail when tool is in forbidden list"
        );
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(
                reason.contains("dangerous_tool"),
                "Error should mention the forbidden tool"
            );
        }
    }

    #[test]
    fn test_no_forbidden_tools_with_full_toolset() {
        let mut ctx = make_test_context();
        ctx.allowed_tools = ToolSet::Full;
        ctx.tools_used = vec!["any_tool".to_string()];

        let guard = super::super::Guard::NoForbiddenToolsUsed;
        let result = check_no_forbidden_tools(&guard, &ctx);
        assert!(
            result.is_ok(),
            "Should pass with Full toolset (no forbidden tools)"
        );
    }

    // ============= check_only_allowed_tools tests =============

    #[test]
    fn test_only_allowed_tools_in_allowlist() {
        let mut ctx = make_test_context();
        ctx.allowed_tools = ToolSet::Allow(vec!["read_tool".to_string(), "write_tool".to_string()]);
        ctx.tools_used = vec!["read_tool".to_string()];

        let guard = super::super::Guard::OnlyAllowedToolsUsed;
        let result = check_only_allowed_tools(&guard, &ctx);
        assert!(
            result.is_ok(),
            "Should pass when tool is in allowed list"
        );
    }

    #[test]
    fn test_only_allowed_tools_not_in_allowlist() {
        let mut ctx = make_test_context();
        ctx.allowed_tools = ToolSet::Allow(vec!["read_tool".to_string(), "write_tool".to_string()]);
        ctx.tools_used = vec!["forbidden_tool".to_string()];

        let guard = super::super::Guard::OnlyAllowedToolsUsed;
        let result = check_only_allowed_tools(&guard, &ctx);
        assert!(
            result.is_err(),
            "Should fail when tool is not in allowed list"
        );
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(
                reason.contains("forbidden_tool"),
                "Error should mention the disallowed tool"
            );
        }
    }

    #[test]
    fn test_only_allowed_tools_multiple_tools_one_fails() {
        let mut ctx = make_test_context();
        ctx.allowed_tools = ToolSet::Allow(vec!["read_tool".to_string()]);
        ctx.tools_used = vec!["read_tool".to_string(), "forbidden_tool".to_string()];

        let guard = super::super::Guard::OnlyAllowedToolsUsed;
        let result = check_only_allowed_tools(&guard, &ctx);
        assert!(
            result.is_err(),
            "Should fail when any tool is not in allowed list"
        );
    }

    // ============= check_shell_denylist tests =============

    #[test]
    fn test_shell_denylist_no_denylisted_patterns() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("ls -la\ncat file.txt\necho hello".to_string()));

        let cmds = vec!["rm -rf".to_string(), "dd if=".to_string()];
        let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
        let result = check_shell_denylist(&guard, &ctx, &cmds);
        assert!(
            result.is_ok(),
            "Should pass when no denylisted patterns found in output"
        );
    }

    #[test]
    fn test_shell_denylist_pattern_found() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("rm -rf /tmp\nls -la".to_string()));

        let cmds = vec!["rm -rf".to_string(), "dd if=".to_string()];
        let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
        let result = check_shell_denylist(&guard, &ctx, &cmds);
        assert!(
            result.is_err(),
            "Should fail when denylisted pattern found in output"
        );
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(
                reason.contains("rm -rf"),
                "Error should mention the denied command"
            );
        }
    }

    #[test]
    fn test_shell_denylist_no_output() {
        let mut ctx = make_test_context();
        ctx.output = None;

        let cmds = vec!["rm -rf".to_string()];
        let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
        let result = check_shell_denylist(&guard, &ctx, &cmds);
        assert!(
            result.is_ok(),
            "Should pass when there is no output to check"
        );
    }

    #[test]
    fn test_shell_allowlist_allowed_prefix() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("git clone repo\ngit push".to_string()));

        let allowed = vec!["git".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_ok(),
            "Should pass when all commands start with allowed prefix"
        );
    }

    #[test]
    fn test_shell_allowlist_disallowed_prefix() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("git clone repo\nrm file".to_string()));

        let allowed = vec!["git".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_err(),
            "Should fail when any command does not start with allowed prefix"
        );
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(
                reason.contains("rm file"),
                "Error should mention the disallowed command"
            );
        }
    }
}
