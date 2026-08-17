use super::GuardError;
use crate::agent::NetworkPolicy;
use crate::context::StepContext;

pub fn check_no_secrets_in_output(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let matches = crate::injection::SecretScanner::scan(&output.raw);
        if matches.is_empty() {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "NoSecretsInOutput".to_string(),
                reason: format!("found {} secret patterns in output", matches.len()),
            })
        }
    } else {
        Ok(())
    }
}

pub fn check_no_secrets_in_diff(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    scan_output_for_secrets(ctx, "NoSecretsInDiff")
}

pub fn check_no_secret_exfiltration(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    scan_output_for_secrets(ctx, "NoSecretExfiltration")
}

pub fn check_no_dangerous_shell_commands(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw.to_lowercase();
        let dangerous = vec!["rm -rf", "dd if=", "mkfs", ":(){:|:&};:", ":(){ :|:& };:"];
        for pattern in dangerous {
            if text.contains(pattern) {
                return Err(GuardError::Failed {
                    guard: "NoDangerousShellCommands".to_string(),
                    reason: format!("dangerous shell command found: {}", pattern),
                });
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}

pub fn check_no_new_network_access(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    match &ctx.network_policy {
        NetworkPolicy::DenyAll => Ok(()),
        NetworkPolicy::AllowList(hosts) if hosts.is_empty() => Ok(()),
        NetworkPolicy::AllowList(hosts) => Err(GuardError::Failed {
            guard: "NoNewNetworkAccess".into(),
            reason: format!(
                "Network access is permitted to {} hosts; expected DenyAll",
                hosts.len()
            ),
        }),
        NetworkPolicy::AllowAll => Err(GuardError::Failed {
            guard: "NoNewNetworkAccess".into(),
            reason: "Network policy is AllowAll — unrestricted network access permitted".into(),
        }),
    }
}

pub fn check_no_permission_escalation(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw;
        let words: Vec<&str> = text
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .collect();

        let has_sudo = words.iter().any(|w| *w == "sudo" || *w == "su");
        if has_sudo {
            return Err(GuardError::Failed {
                guard: "NoPermissionEscalation".to_string(),
                reason: "detected sudo/su command".to_string(),
            });
        }

        let text_lower = text.to_lowercase();
        let dangerous_patterns = vec!["chmod 777", "chmod +s", "chown root", "setuid"];
        for pattern in dangerous_patterns {
            if text_lower.contains(pattern) {
                return Err(GuardError::Failed {
                    guard: "NoPermissionEscalation".to_string(),
                    reason: format!("potential escalation pattern found: {}", pattern),
                });
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}

pub fn check_no_safety_bypass(_guard: &super::Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw.to_lowercase();
        let patterns = vec!["ignore safety", "#[allow(unsafe)]", "unsafe {"];
        for pattern in patterns {
            if text.contains(pattern) {
                return Err(GuardError::Failed {
                    guard: "NoSafetyBypass".to_string(),
                    reason: format!("safety bypass pattern found: {}", pattern),
                });
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}

pub fn check_no_test_disabling(_guard: &super::Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw.to_lowercase();
        let patterns = vec!["#[ignore]", "#[skip]", "skip_test"];
        for pattern in patterns {
            if text.contains(pattern) {
                return Err(GuardError::Failed {
                    guard: "NoTestDisabling".to_string(),
                    reason: format!("test disabling pattern found: {}", pattern),
                });
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}

pub fn check_no_guard_removal(_guard: &super::Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let removes_guard = output.raw.lines().any(|line| {
            line.starts_with('-') && !line.starts_with("---") && line.contains("Guard::")
        });
        if removes_guard {
            Err(GuardError::Failed {
                guard: "NoGuardRemoval".into(),
                reason: "Diff removes Guard:: references — possible guard bypass attempt".into(),
            })
        } else {
            Ok(())
        }
    } else {
        Err(GuardError::Failed {
            guard: "NoGuardRemoval".into(),
            reason: "no output".into(),
        })
    }
}

/// Shared private helper: scan output for secrets above blocking threshold.
/// Fails if any secret at RiskLevel::High or RiskLevel::Critical is detected.
fn scan_output_for_secrets(ctx: &StepContext, guard_name: &str) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let matches = crate::injection::SecretScanner::scan(&output.raw);
        
        // Filter for high-risk secrets (High and Critical levels)
        let blocking_matches: Vec<_> = matches
            .iter()
            .filter(|m| {
                matches!(m.risk_level, Some(crate::injection::RiskLevel::High) | Some(crate::injection::RiskLevel::Critical))
            })
            .collect();
        
        if blocking_matches.is_empty() {
            Ok(())
        } else {
            let details = blocking_matches
                .iter()
                .map(|m| format!("{} ({})", m.pattern_name, m.redacted))
                .collect::<Vec<_>>()
                .join(", ");
            Err(GuardError::Failed {
                guard: guard_name.to_string(),
                reason: format!("detected {} high-risk secret(s): {}", blocking_matches.len(), details),
            })
        }
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::StepContext;
    use crate::action::StepOutput;
    use crate::agent::FilesystemPolicy;
    use serde_json::json;

    fn make_context_with_output(raw: String) -> StepContext {
        let mut ctx = StepContext::new(
            "test_agent".to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            json!({}),
            FilesystemPolicy {
                workspace_root: std::path::PathBuf::from("/tmp"),
                read_paths: vec![],
                write_paths: vec![],
                forbidden_paths: vec![],
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
        );
        ctx.output = Some(StepOutput {
            raw,
            parsed: None,
        });
        ctx
    }

    // Tests for check_no_secrets_in_diff

    #[test]
    fn test_check_no_secrets_in_diff_pass_no_secrets() {
        let ctx = make_context_with_output("--- a/file.rs\n+++ b/file.rs\n@@ -1,3 +1,3 @@\n fn main() {}".to_string());
        let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_no_secrets_in_diff_fail_openai_key() {
        let ctx = make_context_with_output(
            "--- a/.env\n+++ b/.env\n@@ -1 @@\n+OPENAI_KEY=sk-proj-1234567890abcdefghijklmnop".to_string()
        );
        let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
        assert!(result.is_err());
        if let Err(GuardError::Failed { guard, reason }) = result {
            assert_eq!(guard, "NoSecretsInDiff");
            assert!(reason.contains("secret"));
        }
    }

    #[test]
    fn test_check_no_secrets_in_diff_fail_aws_key() {
        let ctx = make_context_with_output(
            "--- a/config.txt\n+++ b/config.txt\n@@ -1 @@\n+AWS_KEY=AKIAIOSFODNN7EXAMPLE".to_string()
        );
        let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
        assert!(result.is_err());
        if let Err(GuardError::Failed { guard, reason }) = result {
            assert_eq!(guard, "NoSecretsInDiff");
            assert!(reason.contains("secret"));
        }
    }

    #[test]
    fn test_check_no_secrets_in_diff_fail_private_key() {
        let ctx = make_context_with_output(
            "--- a/id_rsa\n+++ b/id_rsa\n@@ -1 @@\n+-----BEGIN PRIVATE KEY-----".to_string()
        );
        let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
        assert!(result.is_err());
        if let Err(GuardError::Failed { guard, reason }) = result {
            assert_eq!(guard, "NoSecretsInDiff");
            assert!(reason.contains("secret"));
        }
    }

    #[test]
    fn test_check_no_secrets_in_diff_pass_no_output() {
        let ctx = StepContext::new(
            "test_agent".to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            json!({}),
            FilesystemPolicy {
                workspace_root: std::path::PathBuf::from("/tmp"),
                read_paths: vec![],
                write_paths: vec![],
                forbidden_paths: vec![],
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
        );
        let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
        assert!(result.is_ok());
    }

    // Tests for check_no_secret_exfiltration

    #[test]
    fn test_check_no_secret_exfiltration_pass_no_secrets() {
        let ctx = make_context_with_output("This is a regular output with no sensitive data.".to_string());
        let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_no_secret_exfiltration_fail_openai_key() {
        let ctx = make_context_with_output(
            "API Response: sk-proj-thisIsAFakeButWellFormedOpenAIKey12345".to_string()
        );
        let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
        assert!(result.is_err());
        if let Err(GuardError::Failed { guard, reason }) = result {
            assert_eq!(guard, "NoSecretExfiltration");
            assert!(reason.contains("secret"));
        }
    }

    #[test]
    fn test_check_no_secret_exfiltration_fail_aws_key() {
        let ctx = make_context_with_output(
            "Retrieved credential AKIAIOSFODNN7EXAMPLE from config".to_string()
        );
        let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
        assert!(result.is_err());
        if let Err(GuardError::Failed { guard, reason }) = result {
            assert_eq!(guard, "NoSecretExfiltration");
            assert!(reason.contains("secret"));
        }
    }

    #[test]
    fn test_check_no_secret_exfiltration_fail_private_key() {
        let ctx = make_context_with_output(
            "Key data:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA1234567890abcdef".to_string()
        );
        let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
        assert!(result.is_err());
        if let Err(GuardError::Failed { guard, reason }) = result {
            assert_eq!(guard, "NoSecretExfiltration");
            assert!(reason.contains("secret"));
        }
    }

    #[test]
    fn test_check_no_secret_exfiltration_pass_no_output() {
        let ctx = StepContext::new(
            "test_agent".to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            json!({}),
            FilesystemPolicy {
                workspace_root: std::path::PathBuf::from("/tmp"),
                read_paths: vec![],
                write_paths: vec![],
                forbidden_paths: vec![],
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
        );
        let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_check_no_secret_exfiltration_fail_env_var_secret() {
        let ctx = make_context_with_output(
            "DATABASE_PASSWORD=super_secret_password_123".to_string()
        );
        let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
        assert!(result.is_err());
        if let Err(GuardError::Failed { guard, reason }) = result {
            assert_eq!(guard, "NoSecretExfiltration");
            assert!(reason.contains("secret"));
        }
    }

    #[test]
    fn test_shared_helper_ignores_low_medium_risk() {
        // SecretScanner returns Low/Medium risk for some patterns, but our guard only blocks High/Critical
        // This test verifies that the helper doesn't block on low-risk findings
        let ctx = make_context_with_output("[system]".to_string()); // Low-risk injection pattern
        let result = scan_output_for_secrets(&ctx, "TestGuard");
        assert!(result.is_ok(), "Low-risk patterns should not block the guard");
    }
}
