use super::GuardError;
use crate::context::StepContext;
use crate::toolset::ToolSet;
use std::path::Path;

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
    // Check actual executed shell commands against allowlist
    for (_tool_name, cmd) in &ctx.commands_executed {
        // Extract the first word (the command itself) for matching
        let first_word = cmd.split_whitespace().next().unwrap_or("");
        
        // Normalize the first word to just the basename
        // e.g., "/usr/bin/cargo" → "cargo"
        let first_word_basename = Path::new(first_word)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(first_word);
        
        // Check if this command matches any pattern in the allowlist
        // Use exact match semantics (not prefix matching) to avoid over-permitting
        let allowed = cmds.iter().any(|pattern| {
            // Normalize the pattern to just the basename as well
            let pattern_basename = Path::new(pattern)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(pattern);
            
            // Exact match on the basename (first word)
            first_word_basename == pattern_basename || 
            // Or exact match on the full first word vs the full pattern
            first_word == pattern ||
            // Or if the command starts with the full pattern + space/args
            // (e.g., "cargo test ..." matches allowlist pattern "cargo test")
            cmd.starts_with(&format!("{} ", pattern)) || cmd == pattern
        });

        if !allowed {
            return Err(GuardError::Failed {
                guard: "ShellCommandAllowlist".to_string(),
                reason: format!("shell command '{}' not in allowlist", cmd),
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
    // Check actual executed shell commands against denylist
    for (_tool_name, cmd) in &ctx.commands_executed {
        // Extract the first word (the command itself) for matching
        let first_word = cmd.split_whitespace().next().unwrap_or("");
        
        // Normalize the first word to just the basename
        // e.g., "/usr/bin/rm" → "rm"
        let first_word_basename = Path::new(first_word)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(first_word);
        
        // Check if this command matches any pattern in the denylist
        // Use exact match semantics on the basename (not substring matching)
        let denied = cmds.iter().any(|pattern| {
            // Normalize the pattern to just the basename as well
            let pattern_basename = Path::new(pattern)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(pattern);
            
            // Exact match on the basename
            first_word_basename == pattern_basename || 
            // Or exact match on the full first word vs the full pattern
            first_word == pattern
        });

        if denied {
            return Err(GuardError::Failed {
                guard: "ShellCommandDenylist".to_string(),
                reason: format!("shell command '{}' matches deny pattern", cmd),
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
    fn test_shell_denylist_no_shell_tools_executed() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("some output".to_string()));
        ctx.tools_used = vec!["other_tool".to_string()];
        // No commands actually executed
        ctx.commands_executed = vec![];

        let cmds = vec!["rm".to_string(), "dd".to_string()];
        let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
        let result = check_shell_denylist(&guard, &ctx, &cmds);
        assert!(
            result.is_ok(),
            "Should pass when no shell commands were actually executed"
        );
    }

    #[test]
    fn test_shell_denylist_tool_actually_executed() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("some output".to_string()));
        ctx.tools_used = vec!["shell.run".to_string()];
        // The actual command executed is "rm -rf /"
        ctx.commands_executed = vec![("shell.run".to_string(), "rm -rf /".to_string())];

        let cmds = vec!["rm".to_string(), "dd".to_string()];
        let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
        let result = check_shell_denylist(&guard, &ctx, &cmds);
        assert!(
            result.is_err(),
            "Should fail when a denied shell command was actually executed"
        );
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(
                reason.contains("rm -rf"),
                "Error should mention the actually-executed command"
            );
        }
    }

    #[test]
    fn test_shell_denylist_allowed_command_executed() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("git clone result".to_string()));
        ctx.tools_used = vec!["shell.run".to_string()];
        // The actual command executed is "git clone <url>"
        ctx.commands_executed = vec![("shell.run".to_string(), "git clone https://github.com/example/repo".to_string())];

        let cmds = vec!["rm".to_string(), "dd".to_string()];
        let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
        let result = check_shell_denylist(&guard, &ctx, &cmds);
        assert!(
            result.is_ok(),
            "Should pass when an allowed shell command was executed"
        );
    }

    #[test]
    fn test_shell_allowlist_output_text_irrelevant() {
        let mut ctx = make_test_context();
        // Output contains "rm file" but the actual command executed is "git clone"
        ctx.output = Some(StepOutput::new("git clone result\nrm file (simulated output)".to_string()));
        ctx.tools_used = vec!["shell.run".to_string()];
        // The actual command executed was git, not rm
        ctx.commands_executed = vec![("shell.run".to_string(), "git clone https://github.com/example/repo".to_string())];

        let allowed = vec!["git".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_ok(),
            "Should pass when actually-executed commands are in allowlist, regardless of output text"
        );
    }

    #[test]
    fn test_shell_allowlist_forbidden_command_executed() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("some output".to_string()));
        ctx.tools_used = vec!["shell.run".to_string()];
        // Actual command executed is "curl", which is not in the allowlist
        ctx.commands_executed = vec![("shell.run".to_string(), "curl http://evil.com".to_string())];

        let allowed = vec!["git".to_string(), "cargo".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_err(),
            "Should fail when an actually-executed command is not in allowlist"
        );
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(
                reason.contains("curl"),
                "Error should mention the actually-executed command"
            );
        }
    }

    #[test]
    fn test_shell_allowlist_multiple_commands() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("git and cargo executed".to_string()));
        ctx.tools_used = vec!["shell.run".to_string(), "shell.cargo_test".to_string()];
        ctx.commands_executed = vec![
            ("shell.run".to_string(), "git clone https://github.com/example/repo".to_string()),
            ("shell.cargo_test".to_string(), "cargo test".to_string()),
        ];

        let allowed = vec!["git".to_string(), "cargo".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_ok(),
            "Should pass when all executed shell commands are in allowlist"
        );
    }

    #[test]
    fn test_shell_allowlist_one_command_not_allowed() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("git and rm executed".to_string()));
        ctx.tools_used = vec!["shell.run".to_string(), "shell.run".to_string()];
        ctx.commands_executed = vec![
            ("shell.run".to_string(), "git clone https://github.com/example/repo".to_string()),
            ("shell.run".to_string(), "rm -rf /tmp".to_string()),
        ];

        let allowed = vec!["git".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_err(),
            "Should fail when any executed command is not in allowlist"
        );
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(
                reason.contains("rm"),
                "Error should mention the disallowed command"
            );
        }
    }

    #[test]
    fn test_shell_allowlist_no_shell_commands() {
        let mut ctx = make_test_context();
        ctx.output = Some(StepOutput::new("some output".to_string()));
        ctx.tools_used = vec!["other_tool".to_string()];
        // No shell commands were executed
        ctx.commands_executed = vec![];

        let allowed = vec!["git".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_ok(),
            "Should pass when no shell commands were executed"
        );
    }

    #[test]
    fn test_shell_allowlist_cargo_test_exact_match() {
        let mut ctx = make_test_context();
        ctx.tools_used = vec!["shell.cargo_test".to_string()];
        ctx.commands_executed = vec![("shell.cargo_test".to_string(), "cargo test".to_string())];

        let allowed = vec!["cargo test".to_string()];
        let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
        let result = check_shell_allowlist(&guard, &ctx, &allowed);
        assert!(
            result.is_ok(),
            "Should pass when 'cargo test' command matches full pattern"
        );
    }

    #[test]
    fn test_shell_denylist_no_substring_bypass() {
        let mut ctx = make_test_context();
        ctx.tools_used = vec!["shell.run".to_string()];
        // The command is "cargo test" but denylist has "go_te" (substring of cargo_test)
        // This should NOT match because we use word-boundary matching
        ctx.commands_executed = vec![("shell.run".to_string(), "cargo test".to_string())];

        let cmds = vec!["go_te".to_string()];
        let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
        let result = check_shell_denylist(&guard, &ctx, &cmds);
        assert!(
            result.is_ok(),
            "Should pass: 'go_te' should not match 'cargo' (not a word boundary match)"
        );
    }

     #[test]
     fn test_shell_denylist_critical_rm_bypass() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run".to_string()];
         // Critical test: denylist has "rm" and command is "rm -rf /"
         // This MUST be blocked or the denylist is a no-op
         ctx.commands_executed = vec![("shell.run".to_string(), "rm -rf /".to_string())];

         let cmds = vec!["rm".to_string()];
         let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
         let result = check_shell_denylist(&guard, &ctx, &cmds);
         assert!(
             result.is_err(),
             "CRITICAL: denylist with 'rm' must block 'rm -rf /' command"
         );
         if let Err(GuardError::Failed { reason, .. }) = result {
             assert!(
                 reason.contains("rm"),
                 "Error should mention the 'rm' command"
             );
         }
     }

     // ============= shell.run_command bypass test =============

     #[test]
     fn test_shell_run_command_denylist_blocked() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run_command".to_string()];
         // Critical: shell.run_command with denylisted command must be blocked
         ctx.commands_executed = vec![("shell.run_command".to_string(), "rm -rf /tmp".to_string())];

         let cmds = vec!["rm".to_string()];
         let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
         let result = check_shell_denylist(&guard, &ctx, &cmds);
         assert!(
             result.is_err(),
             "CRITICAL: shell.run_command must respect denylist (was bypassing before fix)"
         );
         if let Err(GuardError::Failed { reason, .. }) = result {
             assert!(
                 reason.contains("rm"),
                 "Error should mention the denylisted command"
             );
         }
     }

     #[test]
     fn test_shell_run_command_allowlist_permitted() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run_command".to_string()];
         ctx.commands_executed = vec![("shell.run_command".to_string(), "git clone https://example.com/repo".to_string())];

         let allowed = vec!["git".to_string()];
         let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
         let result = check_shell_allowlist(&guard, &ctx, &allowed);
         assert!(
             result.is_ok(),
             "shell.run_command should be allowed when command is in allowlist"
         );
     }

     // ============= Absolute path normalization tests =============

     #[test]
     fn test_denylist_absolute_path_rm() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run".to_string()];
         // Command uses absolute path: /usr/bin/rm
         ctx.commands_executed = vec![("shell.run".to_string(), "/usr/bin/rm -rf /tmp".to_string())];

         let cmds = vec!["rm".to_string()];
         let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
         let result = check_shell_denylist(&guard, &ctx, &cmds);
         assert!(
             result.is_err(),
             "Denylist with 'rm' should block '/usr/bin/rm' via basename normalization"
         );
         if let Err(GuardError::Failed { reason, .. }) = result {
             assert!(
                 reason.contains("usr/bin/rm"),
                 "Error should mention the actual command"
             );
         }
     }

     #[test]
     fn test_allowlist_absolute_path_cargo() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run".to_string()];
         // Command uses absolute path: /home/user/.cargo/bin/cargo
         ctx.commands_executed = vec![("shell.run".to_string(), "/home/user/.cargo/bin/cargo build".to_string())];

         let allowed = vec!["cargo".to_string()];
         let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
         let result = check_shell_allowlist(&guard, &ctx, &allowed);
         assert!(
             result.is_ok(),
             "Allowlist with 'cargo' should permit '/home/user/.cargo/bin/cargo' via basename normalization"
         );
     }

     // ============= Exact match (not prefix matching) tests =============

     #[test]
     fn test_allowlist_exact_match_not_prefix() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run".to_string()];
         // Command executed is "echo hello"
         ctx.commands_executed = vec![("shell.run".to_string(), "echo hello".to_string())];

         // Allowlist has "ech" (prefix of "echo")
         let allowed = vec!["ech".to_string()];
         let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
         let result = check_shell_allowlist(&guard, &ctx, &allowed);
         // Should FAIL: "ech" is NOT an exact match for "echo"
         assert!(
             result.is_err(),
             "Allowlist should use exact match, not prefix matching: 'ech' should NOT match 'echo'"
         );
     }

     #[test]
     fn test_allowlist_exact_match_echo_ok() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run".to_string()];
         ctx.commands_executed = vec![("shell.run".to_string(), "echo hello".to_string())];

         // Allowlist has "echo" (exact match)
         let allowed = vec!["echo".to_string()];
         let guard = super::super::Guard::ShellCommandAllowlist(allowed.clone());
         let result = check_shell_allowlist(&guard, &ctx, &allowed);
         assert!(
             result.is_ok(),
             "Allowlist should permit exact match: 'echo' matches 'echo'"
         );
     }

     #[test]
     fn test_denylist_exact_match_not_substring() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run".to_string()];
         // Command executed is "echo hello" (contains "echo")
         ctx.commands_executed = vec![("shell.run".to_string(), "echo hello".to_string())];

         // Denylist has "ho" (substring of "echo")
         let cmds = vec!["ho".to_string()];
         let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
         let result = check_shell_denylist(&guard, &ctx, &cmds);
         // Should PASS: "ho" is NOT an exact match for "echo"
         assert!(
             result.is_ok(),
             "Denylist should use exact match, not substring: 'ho' should NOT match 'echo'"
         );
     }

     #[test]
     fn test_denylist_exact_match_echo_blocked() {
         let mut ctx = make_test_context();
         ctx.tools_used = vec!["shell.run".to_string()];
         ctx.commands_executed = vec![("shell.run".to_string(), "echo hello".to_string())];

         // Denylist has "echo" (exact match)
         let cmds = vec!["echo".to_string()];
         let guard = super::super::Guard::ShellCommandDenylist(cmds.clone());
         let result = check_shell_denylist(&guard, &ctx, &cmds);
         assert!(
             result.is_err(),
             "Denylist should block exact match: 'echo' matches 'echo'"
         );
     }
}
