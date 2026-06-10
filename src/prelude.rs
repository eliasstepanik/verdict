//! Prelude: commonly used types and traits

// Core types
pub use crate::action::{
    DelegationPolicy, IterationFailureMode, ProviderSpec, SkillMode, StepAction, StepError,
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
