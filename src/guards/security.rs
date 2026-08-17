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
#[path = "security_tests.rs"]
mod tests;
