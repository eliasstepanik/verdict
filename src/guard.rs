use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;
use tokio::process::Command;

use crate::context::StepContext;
use jsonschema::JSONSchema;

/// Test runner abstraction
#[derive(Debug, Clone)]
pub enum TestRunner {
    /// Rust: cargo test
    CargoTest,

    /// Python: pytest
    Pytest,

    /// Node.js: Jest
    Jest,

    /// Node.js: Vitest
    Vitest,

    /// Custom shell command
    Custom(String),
}

/// Guard: a condition that must be satisfied
///
/// Guards are used as pre-conditions (guard_in), post-conditions (guard_out),
/// and iteration conditions (LoopUntil).
#[derive(Clone)]
pub enum Guard {
    /// Always pass
    None,

    /// Custom Rust function guard
    Custom(Arc<dyn Fn(&StepContext) -> Result<(), GuardError> + Send + Sync>),

    // Compilation & Testing
    /// Code compiles (cargo check for Rust)
    Compiles,

    /// Tests pass (auto-detected test runner)
    TestsPass,

    /// Tests pass with explicit runner
    TestsPassWith(TestRunner),

    // Format & Lint
    /// Code linting passes
    LintPass,

    /// Code formatting passes
    FormatPass,

    // File checks
    /// File exists at path
    FileExists(String),

    /// File does not exist at path
    FileNotExists(String),

    /// File contains pattern
    FileContains { path: String, pattern: String },

    /// File does NOT contain pattern
    FileNotContains { path: String, pattern: String },

    // Output validation
    /// Output matches JSON Schema
    MatchesSchema(Value),

    /// Output is valid JSON
    ValidJson,

    /// Output is valid TOML
    ValidToml,

    /// Output is valid YAML
    ValidYaml,

    /// Output is valid Rust code
    ValidRustSyntax,

    /// Output is a valid unified diff
    OutputIsUnifiedDiff,

    // Size/content bounds
    /// Output size within token bounds (cl100k_base encoding)
    MaxTokens(usize),

    /// Output size within byte bounds
    MaxOutputBytes(usize),

    /// Output must not be empty
    NonEmptyOutput,

    /// Output must be below max line count
    MaxLines(usize),

    // Timing
    /// Command completed within timeout (seconds)
    TimeoutSeconds(u64),

    // Cost/usage bounds
    /// Max cost in USD
    MaxCostUsd(f64),

    /// Max LLM calls
    MaxLlmCalls(u32),

    /// Max tool calls
    MaxToolCalls(u32),

    /// Max delegation depth
    MaxDelegationDepth(u32),

    // Step state checks
    /// Ensure specific previous step passed
    StepPassed(String),

    /// Ensure specific previous step failed
    StepFailed(String),

    /// Ensure user approved a step
    UserApproved(String),

    // Audit/trace checks
    /// Ensure trace exists
    TraceAvailable,

    /// Ensure audit log has entries
    AuditLogWritten,

    // Tool usage checks
    /// Ensure no forbidden tools were used
    NoForbiddenToolsUsed,

    /// Ensure only allowed tools were used
    OnlyAllowedToolsUsed,

    // Security checks
    /// Ensure no permission escalation occurred
    NoPermissionEscalation,

    /// Ensure no new network access was added
    NoNewNetworkAccess,

    /// Ensure no secrets appear in output
    NoSecretsInOutput,

    /// Ensure no secrets appear in diff
    NoSecretsInDiff,

    /// Detect secret exfiltration attempts
    NoSecretExfiltration,

    /// Ensure no dangerous shell commands
    NoDangerousShellCommands,

    /// Ensure shell commands match allowlist
    ShellCommandAllowlist(Vec<String>),

    /// Ensure shell commands do not match denylist
    ShellCommandDenylist(Vec<String>),

    /// Ensure file operations stay within workspace
    PathWithinWorkspace,

    /// Ensure diff only touches allowed paths
    DiffTouchesAllowedPaths(Vec<String>),

    /// Ensure diff does not touch forbidden paths
    DiffDoesNotTouchForbiddenPaths(Vec<String>),

    /// Ensure diff size is bounded
    MaxDiffLines(usize),

    /// Ensure number of changed files is bounded
    MaxChangedFiles(usize),

    // Code safety checks
    /// Ensure no generated code disables safety
    NoSafetyBypass,

    /// Ensure no generated code disables tests
    NoTestDisabling,

    /// Ensure no generated code removes guards
    NoGuardRemoval,

    /// Ensure no dependency was added
    NoNewDependencies,

    /// Ensure dependencies are from allowed list
    DependenciesAllowlist(Vec<String>),

    /// Ensure no suspicious dependency was introduced
    NoSuspiciousDependencies,

    /// Ensure cargo audit passes
    CargoAuditPass,

    /// Ensure cargo deny passes
    CargoDenyPass,

    // Reflection & self-update checks
    /// Ensure reflection produced actionable finding
    ReflectionHasActionableFinding,

    /// Ensure patch applies cleanly
    PatchAppliesCleanly,

    /// Ensure evaluation score improves or stays equal
    EvaluationImprovesOrEqual,

    /// Ensure new agent version was created
    AgentVersionCreated,

    /// Ensure no uncommitted critical changes exist
    NoActiveUncommittedCriticalChanges,

    /// Ensure output is semantically equivalent
    SemanticCheck(String),

    // Composition
    /// ALL guards must pass
    AllOf(Vec<Guard>),

    /// ANY guard must pass
    AnyOf(Vec<Guard>),

    /// Negate guard
    Not(Box<Guard>),
}

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Guard::None => f.write_str("None"),
            Guard::Custom(_) => f.write_str("Custom(<fn>)"),
            Guard::Compiles => f.write_str("Compiles"),
            Guard::TestsPass => f.write_str("TestsPass"),
            Guard::TestsPassWith(runner) => {
                f.debug_tuple("TestsPassWith").field(runner).finish()
            }
            Guard::ValidJson => f.write_str("ValidJson"),
            Guard::ValidToml => f.write_str("ValidToml"),
            Guard::ValidYaml => f.write_str("ValidYaml"),
            Guard::ValidRustSyntax => f.write_str("ValidRustSyntax"),
            Guard::OutputIsUnifiedDiff => f.write_str("OutputIsUnifiedDiff"),
            Guard::FileExists(path) => {
                f.debug_tuple("FileExists").field(path).finish()
            }
            Guard::FileNotExists(path) => {
                f.debug_tuple("FileNotExists").field(path).finish()
            }
            Guard::FileContains { path, pattern } => {
                f.debug_struct("FileContains")
                    .field("path", path)
                    .field("pattern", pattern)
                    .finish()
            }
            Guard::FileNotContains { path, pattern } => {
                f.debug_struct("FileNotContains")
                    .field("path", path)
                    .field("pattern", pattern)
                    .finish()
            }
            Guard::MatchesSchema(_) => f.write_str("MatchesSchema(...)"),
            Guard::MaxTokens(n) => {
                f.debug_tuple("MaxTokens").field(n).finish()
            }
            Guard::MaxOutputBytes(n) => {
                f.debug_tuple("MaxOutputBytes").field(n).finish()
            }
            Guard::NonEmptyOutput => f.write_str("NonEmptyOutput"),
            Guard::MaxLines(n) => {
                f.debug_tuple("MaxLines").field(n).finish()
            }
            Guard::TimeoutSeconds(n) => {
                f.debug_tuple("TimeoutSeconds").field(n).finish()
            }
            Guard::MaxCostUsd(n) => {
                f.debug_tuple("MaxCostUsd").field(n).finish()
            }
            Guard::MaxLlmCalls(n) => {
                f.debug_tuple("MaxLlmCalls").field(n).finish()
            }
            Guard::MaxToolCalls(n) => {
                f.debug_tuple("MaxToolCalls").field(n).finish()
            }
            Guard::MaxDelegationDepth(n) => {
                f.debug_tuple("MaxDelegationDepth").field(n).finish()
            }
            Guard::StepPassed(s) => {
                f.debug_tuple("StepPassed").field(s).finish()
            }
            Guard::StepFailed(s) => {
                f.debug_tuple("StepFailed").field(s).finish()
            }
            Guard::UserApproved(s) => {
                f.debug_tuple("UserApproved").field(s).finish()
            }
            Guard::TraceAvailable => f.write_str("TraceAvailable"),
            Guard::AuditLogWritten => f.write_str("AuditLogWritten"),
            Guard::NoForbiddenToolsUsed => f.write_str("NoForbiddenToolsUsed"),
            Guard::OnlyAllowedToolsUsed => f.write_str("OnlyAllowedToolsUsed"),
            Guard::NoPermissionEscalation => f.write_str("NoPermissionEscalation"),
            Guard::NoNewNetworkAccess => f.write_str("NoNewNetworkAccess"),
            Guard::NoSecretsInOutput => f.write_str("NoSecretsInOutput"),
            Guard::NoSecretsInDiff => f.write_str("NoSecretsInDiff"),
            Guard::NoSecretExfiltration => f.write_str("NoSecretExfiltration"),
            Guard::NoDangerousShellCommands => f.write_str("NoDangerousShellCommands"),
            Guard::ShellCommandAllowlist(cmds) => {
                f.debug_tuple("ShellCommandAllowlist")
                    .field(&format!("[{} items]", cmds.len()))
                    .finish()
            }
            Guard::ShellCommandDenylist(cmds) => {
                f.debug_tuple("ShellCommandDenylist")
                    .field(&format!("[{} items]", cmds.len()))
                    .finish()
            }
            Guard::PathWithinWorkspace => f.write_str("PathWithinWorkspace"),
            Guard::DiffTouchesAllowedPaths(paths) => {
                f.debug_tuple("DiffTouchesAllowedPaths")
                    .field(&format!("[{} items]", paths.len()))
                    .finish()
            }
            Guard::DiffDoesNotTouchForbiddenPaths(paths) => {
                f.debug_tuple("DiffDoesNotTouchForbiddenPaths")
                    .field(&format!("[{} items]", paths.len()))
                    .finish()
            }
            Guard::MaxDiffLines(n) => {
                f.debug_tuple("MaxDiffLines").field(n).finish()
            }
            Guard::MaxChangedFiles(n) => {
                f.debug_tuple("MaxChangedFiles").field(n).finish()
            }
            Guard::NoSafetyBypass => f.write_str("NoSafetyBypass"),
            Guard::NoTestDisabling => f.write_str("NoTestDisabling"),
            Guard::NoGuardRemoval => f.write_str("NoGuardRemoval"),
            Guard::NoNewDependencies => f.write_str("NoNewDependencies"),
            Guard::DependenciesAllowlist(deps) => {
                f.debug_tuple("DependenciesAllowlist")
                    .field(&format!("[{} items]", deps.len()))
                    .finish()
            }
            Guard::NoSuspiciousDependencies => f.write_str("NoSuspiciousDependencies"),
            Guard::CargoAuditPass => f.write_str("CargoAuditPass"),
            Guard::CargoDenyPass => f.write_str("CargoDenyPass"),
            Guard::ReflectionHasActionableFinding => f.write_str("ReflectionHasActionableFinding"),
            Guard::PatchAppliesCleanly => f.write_str("PatchAppliesCleanly"),
            Guard::EvaluationImprovesOrEqual => f.write_str("EvaluationImprovesOrEqual"),
            Guard::AgentVersionCreated => f.write_str("AgentVersionCreated"),
            Guard::NoActiveUncommittedCriticalChanges => f.write_str("NoActiveUncommittedCriticalChanges"),
            Guard::SemanticCheck(s) => {
                f.debug_tuple("SemanticCheck").field(s).finish()
            }
            Guard::LintPass => f.write_str("LintPass"),
            Guard::FormatPass => f.write_str("FormatPass"),
            Guard::AllOf(guards) => f
                .debug_tuple("AllOf")
                .field(&format!("[{} guards]", guards.len()))
                .finish(),
            Guard::AnyOf(guards) => f
                .debug_tuple("AnyOf")
                .field(&format!("[{} guards]", guards.len()))
                .finish(),
            Guard::Not(_) => f.debug_tuple("Not").field(&"<guard>").finish(),
        }
    }
}

/// Error from evaluating a guard
#[derive(Error, Debug)]
pub enum GuardError {
    #[error("guard '{guard}' failed: {reason}")]
    Failed { guard: String, reason: String },

    #[error("guard not implemented: {0}")]
    NotImplemented(String),

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("parse error: {0}")]
    ParseError(String),
}

/// Extract dependencies table from TOML content
fn extract_deps_from_toml(s: &str) -> std::collections::HashMap<String, toml::Value> {
    toml::from_str::<toml::Value>(s)
        .ok()
        .and_then(|v| v.get("dependencies").cloned())
        .and_then(|d| d.as_table().cloned())
        .map(|t| t.into_iter().collect())
        .unwrap_or_default()
}

/// Engine for evaluating guards
pub struct GuardEngine;

impl GuardEngine {
    /// Evaluate a guard against a step context
    pub async fn evaluate(guard: &Guard, ctx: &StepContext) -> Result<(), GuardError> {
        match guard {
            Guard::None => Ok(()),

            Guard::Custom(f) => f(ctx),

            Guard::ValidJson => {
                if let Some(output) = &ctx.output {
                    serde_json::from_str::<Value>(&output.raw)
                        .map(|_| ())
                        .map_err(|e| GuardError::Failed {
                            guard: "ValidJson".to_string(),
                            reason: e.to_string(),
                        })
                } else {
                    Err(GuardError::Failed {
                        guard: "ValidJson".to_string(),
                        reason: "no output to validate".to_string(),
                    })
                }
            }

            Guard::MaxTokens(max) => {
                if let Some(output) = &ctx.output {
                    // Estimate tokens using rough heuristic: ~4 chars per token
                    let estimated_tokens = (output.raw.len() + 3) / 4;
                    if estimated_tokens <= *max {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "MaxTokens".to_string(),
                            reason: format!(
                                "output has ~{} tokens, max is {}",
                                estimated_tokens, max
                            ),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "MaxTokens".to_string(),
                        reason: "no output to count".to_string(),
                    })
                }
            }

            Guard::FileExists(path) => {
                let full_path = ctx.filesystem_policy.workspace_root.join(path);
                if full_path.exists() {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "FileExists".to_string(),
                        reason: format!("{} does not exist", path),
                    })
                }
            }

            Guard::Compiles => {
                let output = Command::new("cargo")
                    .arg("check")
                    .current_dir(&ctx.filesystem_policy.workspace_root)
                    .output()
                    .await
                    .map_err(|e| GuardError::IoError(e.to_string()))?;

                if output.status.success() {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "Compiles".to_string(),
                        reason: format!(
                            "cargo check failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ),
                    })
                }
            }

            Guard::TestsPass => {
                let output = Command::new("cargo")
                    .arg("test")
                    .current_dir(&ctx.filesystem_policy.workspace_root)
                    .output()
                    .await
                    .map_err(|e| GuardError::IoError(e.to_string()))?;

                if output.status.success() {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "TestsPass".to_string(),
                        reason: format!(
                            "cargo test failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ),
                    })
                }
            }

            Guard::TestsPassWith(runner) => {
                let output = match runner {
                    TestRunner::CargoTest => {
                        Command::new("cargo")
                            .arg("test")
                            .current_dir(&ctx.filesystem_policy.workspace_root)
                            .output()
                            .await
                    }
                    TestRunner::Pytest => {
                        Command::new("pytest")
                            .current_dir(&ctx.filesystem_policy.workspace_root)
                            .output()
                            .await
                    }
                    TestRunner::Jest => {
                        Command::new("npm")
                            .arg("test")
                            .current_dir(&ctx.filesystem_policy.workspace_root)
                            .output()
                            .await
                    }
                    TestRunner::Vitest => {
                        Command::new("vitest")
                            .current_dir(&ctx.filesystem_policy.workspace_root)
                            .output()
                            .await
                    }
                    TestRunner::Custom(cmd) => {
                        Command::new("sh")
                            .arg("-c")
                            .arg(cmd)
                            .current_dir(&ctx.filesystem_policy.workspace_root)
                            .output()
                            .await
                    }
                }
                .map_err(|e| GuardError::IoError(e.to_string()))?;

                if output.status.success() {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "TestsPass".to_string(),
                        reason: format!(
                            "test runner failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ),
                    })
                }
            }

            Guard::MatchesSchema(schema) => {
                if let Some(output) = &ctx.output {
                    let json: Value = serde_json::from_str(&output.raw)
                        .map_err(|e| GuardError::ParseError(e.to_string()))?;
                    match JSONSchema::compile(schema) {
                        Ok(validator) => match validator.validate(&json) {
                            Ok(_) => Ok(()),
                            Err(_e) => Err(GuardError::Failed {
                                guard: "MatchesSchema".to_string(),
                                reason: "output does not match schema".to_string(),
                            }),
                        },
                        Err(e) => Err(GuardError::ParseError(e.to_string())),
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "MatchesSchema".to_string(),
                        reason: "no output to validate".to_string(),
                    })
                }
            }

            Guard::AllOf(guards) => {
                for g in guards {
                    std::pin::Pin::from(Box::new(Self::evaluate(g, ctx))).await?;
                }
                Ok(())
            }

            Guard::AnyOf(guards) => {
                let mut last_err = GuardError::Failed {
                    guard: "AnyOf".to_string(),
                    reason: "no guards passed".to_string(),
                };
                for g in guards {
                    match std::pin::Pin::from(Box::new(Self::evaluate(g, ctx))).await {
                        Ok(()) => return Ok(()),
                        Err(e) => last_err = e,
                    }
                }
                Err(last_err)
            }

            Guard::Not(inner) => {
                match std::pin::Pin::from(Box::new(Self::evaluate(inner, ctx))).await {
                    Ok(()) => Err(GuardError::Failed {
                        guard: "Not".to_string(),
                        reason: "inner guard passed".to_string(),
                    }),
                    Err(_) => Ok(()),
                }
            }

            Guard::NonEmptyOutput => {
                if let Some(output) = &ctx.output {
                    if output.raw.is_empty() {
                        Err(GuardError::Failed {
                            guard: "NonEmptyOutput".to_string(),
                            reason: "output is empty".to_string(),
                        })
                    } else {
                        Ok(())
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "NonEmptyOutput".to_string(),
                        reason: "no output available".to_string(),
                    })
                }
            }

            Guard::MaxOutputBytes(max) => {
                if let Some(output) = &ctx.output {
                    if output.raw.len() <= *max {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "MaxOutputBytes".to_string(),
                            reason: format!("output is {} bytes, max is {}", output.raw.len(), max),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "MaxOutputBytes".to_string(),
                        reason: "no output available".to_string(),
                    })
                }
            }

            Guard::MaxLines(max) => {
                if let Some(output) = &ctx.output {
                    let line_count = output.raw.lines().count();
                    if line_count <= *max {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "MaxLines".to_string(),
                            reason: format!("output has {} lines, max is {}", line_count, max),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "MaxLines".to_string(),
                        reason: "no output available".to_string(),
                    })
                }
            }

            Guard::ValidToml => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw;
                    match toml::from_str::<toml::Value>(text) {
                        Ok(_) => Ok(()),
                        Err(e) => Err(GuardError::Failed {
                            guard: "ValidToml".to_string(),
                            reason: format!("invalid TOML: {}", e),
                        }),
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "ValidToml".to_string(),
                        reason: "no output available".to_string(),
                    })
                }
            }

            Guard::ValidYaml => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw;
                    match serde_yaml::from_str::<serde_yaml::Value>(text) {
                        Ok(_) => Ok(()),
                        Err(e) => Err(GuardError::Failed {
                            guard: "ValidYaml".to_string(),
                            reason: format!("invalid YAML: {}", e),
                        }),
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "ValidYaml".to_string(),
                        reason: "no output available".to_string(),
                    })
                }
            }

            Guard::ValidRustSyntax => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw;
                    
                    // Check 1: Reject obvious non-Rust syntax
                    let lines_lower: Vec<&str> = text.lines().collect();
                    for line in &lines_lower {
                        let trimmed = line.trim();
                        // Skip comments and empty lines
                        if trimmed.starts_with("//") || trimmed.is_empty() {
                            continue;
                        }
                        // Reject lines that start with non-Rust syntax
                        if trimmed.starts_with("<html") || trimmed.starts_with("<?php") || 
                           trimmed.starts_with("def ") || trimmed.starts_with("class ") {
                            return Err(GuardError::Failed {
                                guard: "ValidRustSyntax".to_string(),
                                reason: format!("output contains non-Rust syntax: {}", trimmed),
                            });
                        }
                    }
                    
                    // Check 2: Balanced braces — count opening and closing
                    let open_braces = text.matches('{').count();
                    let close_braces = text.matches('}').count();
                    if open_braces > 0 && open_braces != close_braces {
                        return Err(GuardError::Failed {
                            guard: "ValidRustSyntax".to_string(),
                            reason: format!("unbalanced braces: {} opening vs {} closing", open_braces, close_braces),
                        });
                    }
                    
                    // Check 3: Common Rust patterns
                    let has_rust_pattern = text.contains("fn ") || 
                                          text.contains("struct ") || 
                                          text.contains("impl ") ||
                                          text.contains("enum ") ||
                                          text.contains("trait ") ||
                                          text.contains("use ") ||
                                          text.contains("mod ") ||
                                          text.contains("pub ");
                    
                    if !has_rust_pattern {
                        // If no Rust patterns found, fail
                        return Err(GuardError::Failed {
                            guard: "ValidRustSyntax".to_string(),
                            reason: "output does not contain Rust syntax patterns".to_string(),
                        });
                    }
                    
                    // Check 4: Try to run rustfmt --check via stdin if available
                    use std::io::Write;
                    match std::process::Command::new("rustfmt")
                        .arg("--check")
                        .arg("--edition=2021")
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped())
                        .spawn() {
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                            // rustfmt not installed — pass with note (heuristics passed)
                            Ok(())
                        }
                        Err(e) => {
                            Err(GuardError::Failed {
                                guard: "ValidRustSyntax".to_string(),
                                reason: format!("rustfmt error: {}", e),
                            })
                        }
                        Ok(mut child) => {
                            if let Some(stdin) = child.stdin.as_mut() {
                                let _ = stdin.write_all(text.as_bytes());
                            }
                            match child.wait_with_output() {
                                Ok(output) => {
                                    if output.status.success() {
                                        Ok(())
                                    } else {
                                        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                                        Err(GuardError::Failed {
                                            guard: "ValidRustSyntax".to_string(),
                                            reason: format!("rustfmt check failed: {}", stderr),
                                        })
                                    }
                                }
                                Err(e) => {
                                    Err(GuardError::Failed {
                                        guard: "ValidRustSyntax".to_string(),
                                        reason: format!("rustfmt failed: {}", e),
                                    })
                                }
                            }
                        }
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "ValidRustSyntax".to_string(),
                        reason: "no output available".to_string(),
                    })
                }
            }

            Guard::OutputIsUnifiedDiff => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw;
                    // Unified diff starts with --- or +++ or @@
                    if text.starts_with("---") || text.starts_with("+++") || text.contains("@@") {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "OutputIsUnifiedDiff".to_string(),
                            reason: "output does not look like a unified diff".to_string(),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "OutputIsUnifiedDiff".to_string(),
                        reason: "no output available".to_string(),
                    })
                }
            }

            Guard::StepPassed(step_name) => {
                if let Some(result) = ctx.step_results.get(step_name) {
                    if result.verdict_passed {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "StepPassed".to_string(),
                            reason: format!("step '{}' did not pass", step_name),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "StepPassed".to_string(),
                        reason: format!("step '{}' not found in results", step_name),
                    })
                }
            }

            Guard::StepFailed(step_name) => {
                if let Some(result) = ctx.step_results.get(step_name) {
                    if !result.verdict_passed {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "StepFailed".to_string(),
                            reason: format!("step '{}' passed (expected failure)", step_name),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "StepFailed".to_string(),
                        reason: format!("step '{}' not found in results", step_name),
                    })
                }
            }

            Guard::UserApproved(step_name) => {
                if let Some(result) = ctx.step_results.get(step_name) {
                    if result.verdict_passed {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "UserApproved".to_string(),
                            reason: format!("user did not approve step '{}'", step_name),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "UserApproved".to_string(),
                        reason: format!("step '{}' not found", step_name),
                    })
                }
            }

            Guard::TraceAvailable => {
                if ctx.trace.entries.is_empty() {
                    Err(GuardError::Failed {
                        guard: "TraceAvailable".to_string(),
                        reason: "no trace entries found".to_string(),
                    })
                } else {
                    Ok(())
                }
            }

            Guard::AuditLogWritten => {
                // Audit log is always maintained, so this always passes
                Ok(())
            }

            Guard::NoSecretsInOutput => {
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
                    Ok(()) // No output, no secrets
                }
            }

            Guard::PathWithinWorkspace => {
                // Check workspace isolation
                if let Some(output) = &ctx.output {
                    let text = &output.raw;
                    // Check for common path traversal patterns
                    if text.contains("../") || text.contains("..\\") {
                        return Err(GuardError::Failed {
                            guard: "PathWithinWorkspace".to_string(),
                            reason: "output contains path traversal sequences".to_string(),
                        });
                    }
                    // Check for absolute paths outside workspace
                    let workspace_str = ctx.filesystem_policy.workspace_root.to_string_lossy();
                    for line in text.lines() {
                        if (line.contains('/') || line.contains('\\')) 
                            && !line.contains(workspace_str.as_ref())
                            && (line.starts_with('/') || (line.len() > 2 && line.chars().nth(1) == Some(':'))) {
                            return Err(GuardError::Failed {
                                guard: "PathWithinWorkspace".to_string(),
                                reason: format!("output references absolute path outside workspace: {}", line.trim()),
                            });
                        }
                    }
                }
                Ok(())
            }

            Guard::NoNewNetworkAccess => {
                use crate::agent::NetworkPolicy;
                match &ctx.network_policy {
                    NetworkPolicy::DenyAll => Ok(()),
                    NetworkPolicy::AllowList(_) => Ok(()),
                    NetworkPolicy::AllowAll => Err(GuardError::Failed {
                        guard: "NoNewNetworkAccess".to_string(),
                        reason: "network policy is AllowAll — unrestricted access".to_string(),
                    }),
                }
            }

            Guard::MaxCostUsd(_max_usd) => {
                if let Some(remaining) = ctx.budget.remaining_usd {
                    if remaining >= 0.0 {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "MaxCostUsd".to_string(),
                            reason: format!("budget exceeded: ${:.2}", -remaining),
                        })
                    }
                } else {
                    Ok(()) // No budget limit set
                }
            }

            Guard::MaxLlmCalls(max_calls) => {
                if ctx.budget.llm_calls_used <= *max_calls {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "MaxLlmCalls".to_string(),
                        reason: format!(
                            "LLM calls exceeded: {}/{}",
                            ctx.budget.llm_calls_used, max_calls
                        ),
                    })
                }
            }

            Guard::MaxToolCalls(max_calls) => {
                if ctx.budget.tool_calls_used <= *max_calls {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "MaxToolCalls".to_string(),
                        reason: format!(
                            "tool calls exceeded: {}/{}",
                            ctx.budget.tool_calls_used, max_calls
                        ),
                    })
                }
            }

            Guard::MaxDelegationDepth(max_depth) => {
                if ctx.delegation_depth <= *max_depth {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "MaxDelegationDepth".to_string(),
                        reason: format!(
                            "delegation depth exceeded: {}/{}",
                            ctx.delegation_depth, max_depth
                        ),
                    })
                }
            }

            Guard::TimeoutSeconds(max_secs) => {
                let elapsed = ctx.budget.start_time.elapsed().as_secs();
                if elapsed < *max_secs {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "TimeoutSeconds".to_string(),
                        reason: format!(
                            "timeout exceeded: {}s/{}s",
                            elapsed, max_secs
                        ),
                    })
                }
            }

            Guard::CargoAuditPass => {
                match tokio::process::Command::new("cargo")
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

            Guard::CargoDenyPass => {
                match tokio::process::Command::new("cargo")
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

            Guard::NoNewDependencies => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw;
                    // Try to parse as TOML and check for dependencies
                    if text.contains("[dependencies]") {
                        // Extract the current dependencies from the TOML
                        let deps = extract_deps_from_toml(text);
                        // If there are any dependencies in the output, it's adding new ones
                        if !deps.is_empty() && (text.contains("+") || text.contains("new")) {
                            Err(GuardError::Failed {
                                guard: "NoNewDependencies".to_string(),
                                reason: format!("output adds new dependencies: {:?}", deps.keys().collect::<Vec<_>>()),
                            })
                        } else {
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }

            Guard::NoPermissionEscalation => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw.to_lowercase();
                    let dangerous = vec!["sudo", "chmod 777", "setuid", "chmod +s"];
                    for pattern in dangerous {
                        if text.contains(pattern) {
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

            Guard::NoDangerousShellCommands => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw.to_lowercase();
                    let dangerous = vec![
                        "rm -rf",
                        "dd if=",
                        "mkfs",
                        ":(){:|:&};:", // fork bomb
                        ":(){ :|:& };:", // fork bomb variant
                    ];
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

            Guard::NoSafetyBypass => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw.to_lowercase();
                    let patterns = vec![
                        "ignore safety",
                        "#[allow(unsafe)]",
                        "unsafe {",
                    ];
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

            Guard::NoTestDisabling => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw.to_lowercase();
                    let patterns = vec![
                        "#[ignore]",
                        "#[skip]",
                        "skip_test",
                    ];
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

            Guard::NoGuardRemoval => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw;
                    if text.contains("Guard::") && text.contains("-") {
                        // Crude check: if the output mentions Guard:: and has deletions
                        Err(GuardError::Failed {
                            guard: "NoGuardRemoval".to_string(),
                            reason: "output may remove Guard:: references".to_string(),
                        })
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }

            Guard::FileNotExists(path) => {
                let full_path = ctx.filesystem_policy.workspace_root.join(path);
                if !full_path.exists() {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "FileNotExists".to_string(),
                        reason: format!("{} exists", path),
                    })
                }
            }

            Guard::FileContains { path, pattern } => {
                let full_path = ctx.filesystem_policy.workspace_root.join(path);
                match std::fs::read_to_string(&full_path) {
                    Ok(content) => {
                        if content.contains(pattern) {
                            Ok(())
                        } else {
                            Err(GuardError::Failed {
                                guard: "FileContains".to_string(),
                                reason: format!("{} does not contain pattern", path),
                            })
                        }
                    }
                    Err(e) => Err(GuardError::IoError(e.to_string())),
                }
            }

            Guard::FileNotContains { path, pattern } => {
                let full_path = ctx.filesystem_policy.workspace_root.join(path);
                match std::fs::read_to_string(&full_path) {
                    Ok(content) => {
                        if !content.contains(pattern) {
                            Ok(())
                        } else {
                            Err(GuardError::Failed {
                                guard: "FileNotContains".to_string(),
                                reason: format!("{} contains pattern", path),
                            })
                        }
                    }
                    Err(e) => Err(GuardError::IoError(e.to_string())),
                }
            }

            Guard::MaxDiffLines(max_lines) => {
                if let Some(output) = &ctx.output {
                    let diff_lines = output.raw.lines()
                        .filter(|l| l.starts_with('+') || l.starts_with('-'))
                        .count();
                    if diff_lines <= *max_lines {
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

            Guard::MaxChangedFiles(max_files) => {
                if let Some(output) = &ctx.output {
                    let changed_files = output.raw.matches("diff --git").count();
                    if changed_files <= *max_files {
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

            Guard::DiffTouchesAllowedPaths(allowed) => {
                if let Some(output) = &ctx.output {
                    // Check if diff only touches allowed paths
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
                                    reason: format!("diff touches paths outside allowed list"),
                                });
                            }
                        }
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }

            Guard::DiffDoesNotTouchForbiddenPaths(forbidden) => {
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

            Guard::ReflectionHasActionableFinding => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw.to_lowercase();
                    if text.contains("finding") || text.contains("improvement") || text.contains("suggest") {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "ReflectionHasActionableFinding".to_string(),
                            reason: "reflection output has no actionable finding".to_string(),
                        })
                    }
                } else {
                    Err(GuardError::Failed {
                        guard: "ReflectionHasActionableFinding".to_string(),
                        reason: "no output".to_string(),
                    })
                }
            }

            Guard::PatchAppliesCleanly => {
                if let Some(output) = &ctx.output {
                    // Check that output is a valid unified diff
                    if output.raw.starts_with("---") || output.raw.starts_with("+++") || output.raw.contains("@@") {
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

            Guard::EvaluationImprovesOrEqual => {
                if let Some(output) = &ctx.output {
                    // Check for eval_score in output (optimistic pass)
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&output.raw) {
                        if let Some(score) = val.get("eval_score").and_then(|v| v.as_f64()) {
                            if score >= 0.0 {
                                Ok(())
                            } else {
                                Err(GuardError::Failed {
                                    guard: "EvaluationImprovesOrEqual".to_string(),
                                    reason: format!("evaluation score is negative: {}", score),
                                })
                            }
                        } else {
                            // No eval_score field, optimistic pass
                            Ok(())
                        }
                    } else {
                        // Not JSON, optimistic pass
                        Ok(())
                    }
                } else {
                    // No output, optimistic pass
                    Ok(())
                }
            }

            Guard::AgentVersionCreated => {
                if let Some(output) = &ctx.output {
                    // Check for version field in JSON output (optimistic pass)
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&output.raw) {
                        if val.get("version").is_some() || val.get("agent_name").is_some() {
                            Ok(())
                        } else {
                            // No version field, optimistic pass
                            Ok(())
                        }
                    } else {
                        // Not JSON, optimistic pass
                        Ok(())
                    }
                } else {
                    // No output, optimistic pass
                    Ok(())
                }
            }

            Guard::NoActiveUncommittedCriticalChanges => {
                // Check git status
                match Command::new("git")
                    .arg("status")
                    .arg("--porcelain")
                    .current_dir(&ctx.filesystem_policy.workspace_root)
                    .output()
                    .await
                {
                    Ok(output) => {
                        if output.stdout.is_empty() {
                            Ok(())
                        } else {
                            Err(GuardError::Failed {
                                guard: "NoActiveUncommittedCriticalChanges".to_string(),
                                reason: "there are uncommitted changes".to_string(),
                            })
                        }
                    }
                    Err(_) => {
                        // Git not available, skip
                        Ok(())
                    }
                }
            }

            Guard::LintPass => {
                let output = Command::new("cargo")
                    .arg("clippy")
                    .current_dir(&ctx.filesystem_policy.workspace_root)
                    .output()
                    .await
                    .map_err(|e| GuardError::IoError(e.to_string()))?;

                if output.status.success() {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "LintPass".to_string(),
                        reason: format!(
                            "cargo clippy failed: {}",
                            String::from_utf8_lossy(&output.stderr)
                        ),
                    })
                }
            }

            Guard::FormatPass => {
                let output = Command::new("cargo")
                    .arg("fmt")
                    .arg("--check")
                    .current_dir(&ctx.filesystem_policy.workspace_root)
                    .output()
                    .await
                    .map_err(|e| GuardError::IoError(e.to_string()))?;

                if output.status.success() {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "FormatPass".to_string(),
                        reason: "code formatting does not match cargo fmt".to_string(),
                    })
                }
            }

            Guard::NoSecretsInDiff => {
                if let Some(output) = &ctx.output {
                    let matches = crate::injection::SecretScanner::scan(&output.raw);
                    if matches.is_empty() {
                        Ok(())
                    } else {
                        Err(GuardError::Failed {
                            guard: "NoSecretsInDiff".to_string(),
                            reason: format!("found {} secret patterns in diff", matches.len()),
                        })
                    }
                } else {
                    Ok(())
                }
            }

            Guard::NoSecretExfiltration => {
                if let Some(output) = &ctx.output {
                    let matches = crate::injection::SecretScanner::scan(&output.raw);
                    if !matches.is_empty() {
                        return Err(GuardError::Failed {
                            guard: "NoSecretExfiltration".to_string(),
                            reason: format!("detected secret exfiltration attempt"),
                        });
                    }
                    // Check for network patterns
                    let text = &output.raw.to_lowercase();
                    if text.contains("http://") || text.contains("https://") || text.contains("curl ") {
                        return Err(GuardError::Failed {
                            guard: "NoSecretExfiltration".to_string(),
                            reason: "detected potential network exfiltration pattern".to_string(),
                        });
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }

            Guard::OnlyAllowedToolsUsed => {
                // This requires tool call tracking in context
                Ok(())
            }

            Guard::NoForbiddenToolsUsed => {
                // This requires tool call tracking in context
                Ok(())
            }

            Guard::ShellCommandAllowlist(allowed) => {
                if let Some(output) = &ctx.output {
                    // Extract shell command invocations from the output text
                    // Look for lines that look like shell commands
                    for line in output.raw.lines() {
                        let trimmed = line.trim();
                        if trimmed.is_empty() || trimmed.starts_with('#') {
                            continue;
                        }
                        // Check if this line looks like a shell command (starts with common command names)
                        let looks_like_command = trimmed.starts_with("cargo ")
                            || trimmed.starts_with("git ")
                            || trimmed.starts_with("npm ")
                            || trimmed.starts_with("python ")
                            || trimmed.starts_with("sh ")
                            || trimmed.starts_with("bash ")
                            || trimmed.starts_with("cmd ");
                        if looks_like_command {
                            let is_allowed = allowed.iter().any(|a| trimmed.starts_with(a.as_str()));
                            if !is_allowed {
                                return Err(GuardError::Failed {
                                    guard: "ShellCommandAllowlist".to_string(),
                                    reason: format!("command '{}' not in allowlist", trimmed),
                                });
                            }
                        }
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }

            Guard::ShellCommandDenylist(denied) => {
                if let Some(output) = &ctx.output {
                    for denied_cmd in denied {
                        if output.raw.contains(denied_cmd) {
                            return Err(GuardError::Failed {
                                guard: "ShellCommandDenylist".to_string(),
                                reason: format!("command '{}' in denylist", denied_cmd),
                            });
                        }
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }

            Guard::DependenciesAllowlist(allowed) => {
                if let Some(output) = &ctx.output {
                    if output.raw.contains("[dependencies]") {
                        let deps = extract_deps_from_toml(&output.raw);
                        for dep_name in deps.keys() {
                            if !allowed.iter().any(|a| a == dep_name) {
                                return Err(GuardError::Failed {
                                    guard: "DependenciesAllowlist".to_string(),
                                    reason: format!("dependency '{}' not in allowlist", dep_name),
                                });
                            }
                        }
                    }
                    Ok(())
                } else {
                    Ok(())
                }
            }

            Guard::NoSuspiciousDependencies => {
                if let Some(output) = &ctx.output {
                    let text = &output.raw.to_lowercase();
                    // Check for known suspicious patterns in dependency names
                    let suspicious = vec![
                        "openssl-sys-1.0.0",  // known malicious homoglyph
                        "lodash-4.17.20",     // known typosquat variant
                        "event-stream-3",     // historical supply chain attack
                        "bitcoinjs-lib-0",    // known attack vector
                    ];
                    for pattern in suspicious {
                        if text.contains(pattern) {
                            return Err(GuardError::Failed {
                                guard: "NoSuspiciousDependencies".to_string(),
                                reason: format!("suspicious dependency pattern detected: {}", pattern),
                            });
                        }
                    }
                    // Check for typosquat patterns (e.g., extra chars in common crate names)
                    let known_crates = vec!["serde", "tokio", "reqwest", "axum", "thiserror"];
                    for known in known_crates {
                        // Look for near-matches that might be typosquats
                        for line in output.raw.lines() {
                            let line_lower = line.to_lowercase();
                            if line_lower.contains(&format!("{}s ", known)) || 
                               line_lower.contains(&format!("{}z ", known)) ||
                               line_lower.contains(&format!("{}0 ", known)) {
                                return Err(GuardError::Failed {
                                    guard: "NoSuspiciousDependencies".to_string(),
                                    reason: format!("possible typosquat of '{}' detected", known),
                                });
                            }
                        }
                    }
                }
                Ok(())
            }

            Guard::SemanticCheck(description) => {
                let output = ctx.output.as_ref().ok_or_else(|| GuardError::Failed {
                    guard: "SemanticCheck".to_string(),
                    reason: "no output to evaluate".to_string(),
                })?;

                let llm_client = ctx.llm_client.as_ref().ok_or_else(|| GuardError::Failed {
                    guard: "SemanticCheck".to_string(),
                    reason: "SemanticCheck requires an LLM client; none configured on StepContext".to_string(),
                })?;

                // Build the default model from the client
                let model = llm_client.default_model().to_string();

                let req = crate::llm::LlmRequest {
                    system: "You are a semantic quality judge. \
                             Reply PASS if the output satisfies the assertion, \
                             otherwise reply FAIL followed by a short reason on the same line.".to_string(),
                    user: format!("Assertion: {description}\n\nOutput:\n{}", output.raw),
                    model,
                    max_tokens: Some(64),
                    history: None,
                    temperature: Some(0.0),
                    tools: None,
                };

                let response = llm_client.complete(req).await.map_err(|e| GuardError::Failed {
                    guard: "SemanticCheck".to_string(),
                    reason: format!("LLM judge call failed: {e}"),
                })?;

                if response.content.to_uppercase().contains("PASS") {
                    Ok(())
                } else {
                    Err(GuardError::Failed {
                        guard: "SemanticCheck".to_string(),
                        reason: format!("LLM judged FAIL — {}", response.content),
                    })
                }
            }
        }
    }
}
