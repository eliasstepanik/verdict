use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

use crate::context::StepContext;

pub mod budget;
pub mod compilation;
pub mod delegation;
pub mod dependencies;
pub mod diff;
pub mod engine;
pub mod filesystem;
pub mod output;
pub mod security;
pub mod self_improve;
pub mod step_state;
pub mod tools;
pub mod trace;

// Re-export from submodules
pub use engine::{GuardEngine, GuardPhase};

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

    /// Only allowed agents were called during delegation
    OnlyAllowedAgentsUsed,

    /// Delegation depth hasn't looped back to same agent
    NoRecursiveDelegation,

    /// A specific delegated agent passed
    DelegatedAgentPassed(String),

    // Step state checks
    /// Ensure specific previous step passed
    StepPassed(String),

    /// Ensure specific previous step failed
    StepFailed(String),

    /// Ensure user approved a step
    UserApproved(String),

    /// Verify previous step's output matches a JSON Schema
    PreviousStepMatchesSchema { step_name: String, schema: Value },

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

    // Session-scoped guards (Phase 13)
    /// Session must not have exceeded its turn limit
    SessionTurnLimit(u32),

    /// Session must not have been idle longer than N seconds
    SessionIdleTimeout(u64),

    /// Session total token usage must be within the given limit
    SessionBudgetWithin { max_tokens: u32 },

    // Cancellation (Phase 14)
    /// Passes if the cancellation token is not cancelled (i.e., execution ran cleanly to this point)
    CancellationCleanupComplete,

    // Server/Daemon Mode (Phase 15)
    /// Passes if server is authenticated (stub for policy checks done at server level)
    ServerAuthValid,
    /// Passes if concurrent sessions are below limit
    ServerConcurrencyWithin(usize),

    // Phase 16: Prompt Templates and Structured Output
    /// Passes if a prompt template renders without error
    PromptTemplateRendered,
    /// Passes if structured output is present in step output
    StructuredOutputPresent,

    // Phase D: Multi-Agent & Orchestration
    /// Passes if a detached agent (by name) has completed (Phase D3)
    DetachedAgentCompleted(String),

    /// Validates resume_data JSON against provided schema (Phase D4)
    ResumeDataMatchesSchema(Value),

    // Composition
    /// ALL guards must pass
    AllOf(Vec<Guard>),

    /// ALL guards must pass, collecting all errors instead of short-circuiting (A5)
    AllOfCollect(Vec<Guard>),

    /// ANY guard must pass
    AnyOf(Vec<Guard>),

    /// Negate guard
    Not(Box<Guard>),
}

impl Guard {
    /// Get a short name for this guard (for audit logging)
    pub fn name(&self) -> String {
        match self {
            Guard::None => "None".to_string(),
            Guard::Custom(_) => "Custom".to_string(),
            Guard::Compiles => "Compiles".to_string(),
            Guard::TestsPass => "TestsPass".to_string(),
            Guard::TestsPassWith(_) => "TestsPassWith".to_string(),
            Guard::LintPass => "LintPass".to_string(),
            Guard::FormatPass => "FormatPass".to_string(),
            Guard::FileExists(_) => "FileExists".to_string(),
            Guard::FileNotExists(_) => "FileNotExists".to_string(),
            Guard::FileContains { .. } => "FileContains".to_string(),
            Guard::FileNotContains { .. } => "FileNotContains".to_string(),
            Guard::MatchesSchema(_) => "MatchesSchema".to_string(),
            Guard::ValidJson => "ValidJson".to_string(),
            Guard::ValidToml => "ValidToml".to_string(),
            Guard::ValidYaml => "ValidYaml".to_string(),
            Guard::ValidRustSyntax => "ValidRustSyntax".to_string(),
            Guard::OutputIsUnifiedDiff => "OutputIsUnifiedDiff".to_string(),
            Guard::MaxTokens(_) => "MaxTokens".to_string(),
            Guard::MaxOutputBytes(_) => "MaxOutputBytes".to_string(),
            Guard::NonEmptyOutput => "NonEmptyOutput".to_string(),
            Guard::MaxLines(_) => "MaxLines".to_string(),
            Guard::TimeoutSeconds(_) => "TimeoutSeconds".to_string(),
            Guard::MaxCostUsd(_) => "MaxCostUsd".to_string(),
            Guard::MaxLlmCalls(_) => "MaxLlmCalls".to_string(),
            Guard::MaxToolCalls(_) => "MaxToolCalls".to_string(),
            Guard::MaxDelegationDepth(_) => "MaxDelegationDepth".to_string(),
            Guard::OnlyAllowedAgentsUsed => "OnlyAllowedAgentsUsed".to_string(),
            Guard::NoRecursiveDelegation => "NoRecursiveDelegation".to_string(),
            Guard::DelegatedAgentPassed(_) => "DelegatedAgentPassed".to_string(),
            Guard::StepPassed(_) => "StepPassed".to_string(),
            Guard::StepFailed(_) => "StepFailed".to_string(),
            Guard::UserApproved(_) => "UserApproved".to_string(),
            Guard::PreviousStepMatchesSchema { .. } => "PreviousStepMatchesSchema".to_string(),
            Guard::TraceAvailable => "TraceAvailable".to_string(),
            Guard::AuditLogWritten => "AuditLogWritten".to_string(),
            Guard::NoForbiddenToolsUsed => "NoForbiddenToolsUsed".to_string(),
            Guard::OnlyAllowedToolsUsed => "OnlyAllowedToolsUsed".to_string(),
            Guard::NoPermissionEscalation => "NoPermissionEscalation".to_string(),
            Guard::NoNewNetworkAccess => "NoNewNetworkAccess".to_string(),
            Guard::NoSecretsInOutput => "NoSecretsInOutput".to_string(),
            Guard::NoSecretsInDiff => "NoSecretsInDiff".to_string(),
            Guard::NoSecretExfiltration => "NoSecretExfiltration".to_string(),
            Guard::NoDangerousShellCommands => "NoDangerousShellCommands".to_string(),
            Guard::ShellCommandAllowlist(_) => "ShellCommandAllowlist".to_string(),
            Guard::ShellCommandDenylist(_) => "ShellCommandDenylist".to_string(),
            Guard::PathWithinWorkspace => "PathWithinWorkspace".to_string(),
            Guard::DiffTouchesAllowedPaths(_) => "DiffTouchesAllowedPaths".to_string(),
            Guard::DiffDoesNotTouchForbiddenPaths(_) => {
                "DiffDoesNotTouchForbiddenPaths".to_string()
            }
            Guard::MaxDiffLines(_) => "MaxDiffLines".to_string(),
            Guard::MaxChangedFiles(_) => "MaxChangedFiles".to_string(),
            Guard::NoSafetyBypass => "NoSafetyBypass".to_string(),
            Guard::NoTestDisabling => "NoTestDisabling".to_string(),
            Guard::NoGuardRemoval => "NoGuardRemoval".to_string(),
            Guard::NoNewDependencies => "NoNewDependencies".to_string(),
            Guard::DependenciesAllowlist(_) => "DependenciesAllowlist".to_string(),
            Guard::NoSuspiciousDependencies => "NoSuspiciousDependencies".to_string(),
            Guard::CargoAuditPass => "CargoAuditPass".to_string(),
            Guard::CargoDenyPass => "CargoDenyPass".to_string(),
            Guard::ReflectionHasActionableFinding => "ReflectionHasActionableFinding".to_string(),
            Guard::PatchAppliesCleanly => "PatchAppliesCleanly".to_string(),
            Guard::EvaluationImprovesOrEqual => "EvaluationImprovesOrEqual".to_string(),
            Guard::AgentVersionCreated => "AgentVersionCreated".to_string(),
            Guard::NoActiveUncommittedCriticalChanges => {
                "NoActiveUncommittedCriticalChanges".to_string()
            }
            Guard::SemanticCheck(_) => "SemanticCheck".to_string(),
            Guard::SessionTurnLimit(_) => "SessionTurnLimit".to_string(),
            Guard::SessionIdleTimeout(_) => "SessionIdleTimeout".to_string(),
            Guard::SessionBudgetWithin { .. } => "SessionBudgetWithin".to_string(),
            Guard::CancellationCleanupComplete => "CancellationCleanupComplete".to_string(),
            Guard::ServerAuthValid => "ServerAuthValid".to_string(),
            Guard::ServerConcurrencyWithin(_) => "ServerConcurrencyWithin".to_string(),
            Guard::PromptTemplateRendered => "PromptTemplateRendered".to_string(),
            Guard::StructuredOutputPresent => "StructuredOutputPresent".to_string(),
            Guard::DetachedAgentCompleted(_) => "DetachedAgentCompleted".to_string(),
            Guard::ResumeDataMatchesSchema(_) => "ResumeDataMatchesSchema".to_string(),
            Guard::AllOf(_) => "AllOf".to_string(),
            Guard::AllOfCollect(_) => "AllOfCollect".to_string(),
            Guard::AnyOf(_) => "AnyOf".to_string(),
            Guard::Not(_) => "Not".to_string(),
        }
    }
}

impl std::fmt::Debug for Guard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Guard::None => f.write_str("None"),
            Guard::Custom(_) => f.write_str("Custom(<fn>)"),
            Guard::Compiles => f.write_str("Compiles"),
            Guard::TestsPass => f.write_str("TestsPass"),
            Guard::TestsPassWith(runner) => f.debug_tuple("TestsPassWith").field(runner).finish(),
            Guard::ValidJson => f.write_str("ValidJson"),
            Guard::ValidToml => f.write_str("ValidToml"),
            Guard::ValidYaml => f.write_str("ValidYaml"),
            Guard::ValidRustSyntax => f.write_str("ValidRustSyntax"),
            Guard::OutputIsUnifiedDiff => f.write_str("OutputIsUnifiedDiff"),
            Guard::FileExists(path) => f.debug_tuple("FileExists").field(path).finish(),
            Guard::FileNotExists(path) => f.debug_tuple("FileNotExists").field(path).finish(),
            Guard::FileContains { path, pattern } => f
                .debug_struct("FileContains")
                .field("path", path)
                .field("pattern", pattern)
                .finish(),
            Guard::FileNotContains { path, pattern } => f
                .debug_struct("FileNotContains")
                .field("path", path)
                .field("pattern", pattern)
                .finish(),
            Guard::MatchesSchema(_) => f.write_str("MatchesSchema(...)"),
            Guard::MaxTokens(n) => f.debug_tuple("MaxTokens").field(n).finish(),
            Guard::MaxOutputBytes(n) => f.debug_tuple("MaxOutputBytes").field(n).finish(),
            Guard::NonEmptyOutput => f.write_str("NonEmptyOutput"),
            Guard::MaxLines(n) => f.debug_tuple("MaxLines").field(n).finish(),
            Guard::TimeoutSeconds(n) => f.debug_tuple("TimeoutSeconds").field(n).finish(),
            Guard::MaxCostUsd(n) => f.debug_tuple("MaxCostUsd").field(n).finish(),
            Guard::MaxLlmCalls(n) => f.debug_tuple("MaxLlmCalls").field(n).finish(),
            Guard::MaxToolCalls(n) => f.debug_tuple("MaxToolCalls").field(n).finish(),
            Guard::MaxDelegationDepth(n) => f.debug_tuple("MaxDelegationDepth").field(n).finish(),
            Guard::StepPassed(s) => f.debug_tuple("StepPassed").field(s).finish(),
            Guard::StepFailed(s) => f.debug_tuple("StepFailed").field(s).finish(),
            Guard::UserApproved(s) => f.debug_tuple("UserApproved").field(s).finish(),
            Guard::PreviousStepMatchesSchema { step_name, schema } => f
                .debug_struct("PreviousStepMatchesSchema")
                .field("step_name", step_name)
                .field("schema", schema)
                .finish(),
            Guard::OnlyAllowedAgentsUsed => f.write_str("OnlyAllowedAgentsUsed"),
            Guard::NoRecursiveDelegation => f.write_str("NoRecursiveDelegation"),
            Guard::DelegatedAgentPassed(agent) => {
                f.debug_tuple("DelegatedAgentPassed").field(agent).finish()
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
            Guard::ShellCommandAllowlist(cmds) => f
                .debug_tuple("ShellCommandAllowlist")
                .field(&format!("[{} items]", cmds.len()))
                .finish(),
            Guard::ShellCommandDenylist(cmds) => f
                .debug_tuple("ShellCommandDenylist")
                .field(&format!("[{} items]", cmds.len()))
                .finish(),
            Guard::PathWithinWorkspace => f.write_str("PathWithinWorkspace"),
            Guard::DiffTouchesAllowedPaths(paths) => f
                .debug_tuple("DiffTouchesAllowedPaths")
                .field(&format!("[{} items]", paths.len()))
                .finish(),
            Guard::DiffDoesNotTouchForbiddenPaths(paths) => f
                .debug_tuple("DiffDoesNotTouchForbiddenPaths")
                .field(&format!("[{} items]", paths.len()))
                .finish(),
            Guard::MaxDiffLines(n) => f.debug_tuple("MaxDiffLines").field(n).finish(),
            Guard::MaxChangedFiles(n) => f.debug_tuple("MaxChangedFiles").field(n).finish(),
            Guard::NoSafetyBypass => f.write_str("NoSafetyBypass"),
            Guard::NoTestDisabling => f.write_str("NoTestDisabling"),
            Guard::NoGuardRemoval => f.write_str("NoGuardRemoval"),
            Guard::NoNewDependencies => f.write_str("NoNewDependencies"),
            Guard::DependenciesAllowlist(deps) => f
                .debug_tuple("DependenciesAllowlist")
                .field(&format!("[{} items]", deps.len()))
                .finish(),
            Guard::NoSuspiciousDependencies => f.write_str("NoSuspiciousDependencies"),
            Guard::CargoAuditPass => f.write_str("CargoAuditPass"),
            Guard::CargoDenyPass => f.write_str("CargoDenyPass"),
            Guard::ReflectionHasActionableFinding => f.write_str("ReflectionHasActionableFinding"),
            Guard::PatchAppliesCleanly => f.write_str("PatchAppliesCleanly"),
            Guard::EvaluationImprovesOrEqual => f.write_str("EvaluationImprovesOrEqual"),
            Guard::AgentVersionCreated => f.write_str("AgentVersionCreated"),
            Guard::NoActiveUncommittedCriticalChanges => {
                f.write_str("NoActiveUncommittedCriticalChanges")
            }
            Guard::SemanticCheck(s) => f.debug_tuple("SemanticCheck").field(s).finish(),
            Guard::SessionTurnLimit(n) => f.debug_tuple("SessionTurnLimit").field(n).finish(),
            Guard::SessionIdleTimeout(secs) => {
                f.debug_tuple("SessionIdleTimeout").field(secs).finish()
            }
            Guard::SessionBudgetWithin { max_tokens } => f
                .debug_struct("SessionBudgetWithin")
                .field("max_tokens", max_tokens)
                .finish(),
            Guard::CancellationCleanupComplete => f.write_str("CancellationCleanupComplete"),
            Guard::ServerAuthValid => f.write_str("ServerAuthValid"),
            Guard::ServerConcurrencyWithin(n) => {
                f.debug_tuple("ServerConcurrencyWithin").field(n).finish()
            }
            Guard::PromptTemplateRendered => f.write_str("PromptTemplateRendered"),
            Guard::StructuredOutputPresent => f.write_str("StructuredOutputPresent"),
            Guard::ResumeDataMatchesSchema(_schema) => f
                .debug_tuple("ResumeDataMatchesSchema")
                .field(&"<schema>")
                .finish(),
            Guard::DetachedAgentCompleted(agent) => f
                .debug_tuple("DetachedAgentCompleted")
                .field(agent)
                .finish(),
            Guard::LintPass => f.write_str("LintPass"),
            Guard::FormatPass => f.write_str("FormatPass"),
            Guard::AllOf(guards) => f
                .debug_tuple("AllOf")
                .field(&format!("[{} guards]", guards.len()))
                .finish(),
            Guard::AllOfCollect(guards) => f
                .debug_tuple("AllOfCollect")
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

    #[error("multiple guard failures: {} errors", .0.len())]
    Multiple(Vec<GuardError>),
}
