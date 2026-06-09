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

            // All other guards are not implemented in Phase 1
            other => Err(GuardError::NotImplemented(format!("{:?}", other))),
        }
    }
}
