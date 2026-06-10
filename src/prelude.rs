//! Prelude: commonly used types and traits

// Core types
pub use crate::action::{
    DelegationPolicy, IterationFailureMode, ProviderSpec, SkillMode, StepAction, StepError,
    StepOutput,
};
pub use crate::agent::{
    Agent, AgentPolicy, AgentVersion, FilesystemPolicy, NetworkPolicy, SkillSet,
    WorkspaceIsolation,
};
pub use crate::audit::{AuditEntry, AuditEvent, AuditLog};
pub use crate::context::{BudgetState, PipelineTrace, StepContext, StepResult, TraceEntry};
pub use crate::guard::{Guard, GuardEngine, GuardError, TestRunner};
pub use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
pub use crate::registry::{AgentRegistry, SkillRegistry, ToolRegistry};
pub use crate::runner::{GuardPhase, PipelineError, PipelineResult, PipelineRunner};
pub use crate::toolset::ToolSet;
pub use crate::tools::{Tool, ToolContext, ToolError, ToolOutput, ToolSource, FunctionTool};
pub use crate::verdict::{Verdict, VerdictEngine, VerdictError};
