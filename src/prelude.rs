//! Prelude: commonly used types and traits

// Phase 10: Stub Completion
pub use crate::llm::{LlmProvider, LlmClient, LlmRequest, LlmResponse, LlmError, ProviderSpec};

// Phase 9: Advanced Execution
pub use crate::action::RemoteAgentError;
pub use crate::agent::RemoteAgentClient;
pub use crate::pipeline::{HotReloadHandle, Plugin, PluginError, PluginRegistry};
pub use crate::audit::MonitoringServer;

// Phase 8: Self-Improvement
pub use crate::eval::{
    EvaluationSuite, EvaluationCase, EvaluationExpected, EvaluationResult, EvaluationSuiteResult,
    EvaluationRunner, EvalError,
};
pub use crate::self_update::{
    SelfUpdateConfig, SelfUpdateProposal, SelfUpdateResult, SelfUpdateEngine, SelfUpdateError,
};

// Phase 7: Safety and Production
pub use crate::injection::{InjectionScanner, InjectionResult, SecretScanner, SecretMatch, RiskLevel};
pub use crate::budget::{BudgetTracker, RateLimiter, BudgetError};

// Core types
pub use crate::action::{
    DelegationPolicy, IterationFailureMode, SkillMode, StepAction, StepError,
    StepOutput,
};
pub use crate::agent::{
    Agent, AgentPolicy, AgentVersion, FilesystemPolicy, NetworkPolicy,
    WorkspaceIsolation,
};
pub use crate::skills::{Skill, SkillExample, SkillEval, SkillSet};
pub use crate::audit::{AuditEntry, AuditEvent, AuditLog};
pub use crate::context::{BudgetState, PipelineTrace, StepContext, StepResult, TraceEntry};
pub use crate::guard::{Guard, GuardEngine, GuardError, TestRunner};
pub use crate::mcp::{McpClient, McpError, McpServerConfig, McpToolAdapter, DiscoveredTool};
pub use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
pub use crate::registry::{AgentRegistry, SkillRegistry, ToolRegistry};
pub use crate::skills::builtin::{api_design, code_review, refactoring, rust_debugging, test_writing};
pub use crate::runner::{GuardPhase, PipelineError, PipelineResult, PipelineRunner};
pub use crate::toolset::ToolSet;
pub use crate::tools::{Tool, ToolContext, ToolError, ToolOutput, ToolSource, FunctionTool};
pub use crate::verdict::{Verdict, VerdictEngine, VerdictError};
pub use crate::agents::{
    planner_agent, coder_agent, reviewer_agent, debugger_agent, reflector_agent, orchestrator_agent,
};
