use serde_json::Value;

use crate::action::StepAction;
use crate::guard::Guard;
use crate::verdict::Verdict;
use crate::toolset::ToolSet;

/// Injection protection strategy for a step
#[derive(Debug, Clone)]
pub enum InjectionProtection {
    /// No protection (Phase 2+: would scan for injection patterns)
    None,

    /// Strict protection (Phase 2+: would apply aggressive filtering)
    Strict,
}

/// How to handle step failure
#[derive(Debug, Clone)]
pub enum FailureMode {
    /// Stop the pipeline immediately
    Abort,

    /// Retry the step up to max_retries times
    Retry,

    /// Skip this step and continue to the next
    Skip,

    /// Execute a fallback pipeline
    Fallback(Box<Pipeline>),
}

/// A single step in a pipeline
#[derive(Debug, Clone)]
pub struct AgentStep {
    /// Step name
    pub name: String,

    /// Guard that must pass before executing this step
    pub guard_in: Guard,

    /// The action to execute
    pub action: StepAction,

    /// Guard that must pass after execution (before verdict)
    pub guard_out: Guard,

    /// Final verdict decision
    pub verdict: Verdict,

    /// Allowed tools for this step
    pub tools: ToolSet,

    /// Injection protection strategy
    pub injection_protection: InjectionProtection,

    /// Expected output schema (for validation and handoff)
    pub output_schema: Option<Value>,
}

/// A composable pipeline of steps
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Pipeline name
    pub name: String,

    /// Ordered sequence of steps
    pub steps: Vec<AgentStep>,

    /// How to handle step failures
    pub on_failure: FailureMode,

    /// Maximum retries per step
    pub max_retries: u32,
}
