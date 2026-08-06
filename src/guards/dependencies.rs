use super::GuardError;
use crate::context::StepContext;
use tokio::process::Command;

pub fn check_no_new_dependencies(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    if let Some(output) = &ctx.output {
        let text = &output.raw;
        let mut in_cargo_toml_diff = false;
        let mut in_dependencies_section = false;

        for line in text.lines() {
            if line.starts_with("--- a/Cargo.toml") || line.starts_with("+++ b/Cargo.toml") {
                in_cargo_toml_diff = true;
                in_dependencies_section = false;
                continue;
            }

            if line.starts_with("diff --git") {
                in_cargo_toml_diff = false;
                in_dependencies_section = false;
                continue;
            }

            if !in_cargo_toml_diff {
                continue;
            }

            if line.contains("[dependencies]") {
                in_dependencies_section = true;
                continue;
            }

            if line.starts_with("[") && !line.contains("[dependencies]") {
                in_dependencies_section = false;
                continue;
            }

            if in_dependencies_section && line.starts_with("+") && !line.starts_with("+++") {
                return Err(GuardError::Failed {
                    guard: "NoNewDependencies".to_string(),
                    reason: format!("diff adds new dependency: {}", line.trim_start_matches('+')),
                });
            }
        }
        Ok(())
    } else {
        Ok(())
    }
}

pub fn check_dependencies_allowlist(
    _guard: &super::Guard,
    ctx: &StepContext,
    allowed: &[String],
) -> Result<(), GuardError> {
    let output = match &ctx.output {
        Some(o) => &o.raw,
        None => return Ok(()),
    };

    let mut in_deps = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed == "[dependencies]" {
            in_deps = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[dependencies]" {
            in_deps = false;
            continue;
        }
        if !in_deps || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Extract crate name: everything before first '='
        if let Some(name) = trimmed.split('=').next() {
            let name = name.trim();
            if !name.is_empty() && !allowed.iter().any(|a| a == name) {
                return Err(GuardError::Failed {
                    guard: "DependenciesAllowlist".to_string(),
                    reason: format!("dependency '{}' not in allowlist", name),
                });
            }
        }
    }
    Ok(())
}

pub fn check_no_suspicious_dependencies(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    let output = match &ctx.output {
        Some(o) => &o.raw,
        None => return Ok(()),
    };

    // Known suspicious patterns: crate names with embedded version numbers
    // e.g., "openssl-sys-1.0.0" is suspicious (typosquatting of "openssl-sys")
    // Detect pattern: name contains a segment like "-1." or "-2." (digit-dot after hyphen)
    let has_version_in_name = |name: &str| -> bool {
        let bytes = name.as_bytes();
        for i in 0..bytes.len().saturating_sub(2) {
            if bytes[i] == b'-' && bytes[i + 1].is_ascii_digit() {
                // Check if there's a dot after the digits
                let mut j = i + 1;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'.' {
                    return true;
                }
            }
        }
        false
    };

    // Also check known malicious package names
    let known_suspicious = ["malware", "cryptominer", "exfil"];

    for line in output.lines() {
        let trimmed = line.trim();
        // Skip section headers, empty lines, comments
        if trimmed.starts_with('[') || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some(name) = trimmed.split('=').next() {
            let name = name.trim();
            if name.is_empty() {
                continue;
            }
            // Check for embedded version numbers in crate name
            if has_version_in_name(name) {
                return Err(GuardError::Failed {
                    guard: "NoSuspiciousDependencies".to_string(),
                    reason: format!("suspicious dependency name '{}' contains version string (possible typosquatting)", name),
                });
            }
            // Check known malicious names
            for bad in &known_suspicious {
                if name.contains(bad) {
                    return Err(GuardError::Failed {
                        guard: "NoSuspiciousDependencies".to_string(),
                        reason: format!(
                            "dependency '{}' matches known suspicious pattern '{}'",
                            name, bad
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

pub async fn check_cargo_audit_pass(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    match Command::new("cargo")
        .arg("audit")
        .arg("--quiet")
        .output()
        .await
    {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                Err(GuardError::Failed {
                    guard: "CargoAuditPass".to_string(),
                    reason: String::from_utf8_lossy(&output.stderr).to_string(),
                })
            }
        }
        Err(_) => Err(GuardError::NotImplemented(
            "cargo audit not installed".to_string(),
        )),
    }
}

pub async fn check_cargo_deny_pass(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    match Command::new("cargo")
        .arg("deny")
        .arg("check")
        .output()
        .await
    {
        Ok(output) => {
            if output.status.success() {
                Ok(())
            } else {
                Err(GuardError::Failed {
                    guard: "CargoDenyPass".to_string(),
                    reason: String::from_utf8_lossy(&output.stderr).to_string(),
                })
            }
        }
        Err(_) => Err(GuardError::NotImplemented(
            "cargo deny not installed".to_string(),
        )),
    }
}
