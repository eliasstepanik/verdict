use crate::action::StepError;
use crate::guards::{GuardError, GuardPhase};
use crate::verdict::VerdictError;
use thiserror::Error;

/// Error from pipeline execution
#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("step '{step}' failed: {error}")]
    StepFailed { step: String, error: StepError },

    #[error("max retries exceeded for step '{step}'")]
    MaxRetriesExceeded { step: String },

    #[error("guard failed at step '{step}' ({phase:?}): {error}")]
    GuardFailed {
        step: String,
        phase: GuardPhase,
        error: GuardError,
    },

    #[error("verdict failed at step '{step}': {error}")]
    VerdictFailed { step: String, error: VerdictError },

    #[error("awaiting approval at step '{step}': {prompt}")]
    AwaitingApproval { step: String, prompt: &'static str },

    #[error("delegation failed at step '{step}' (agent '{agent}'): {reason}")]
    DelegationFailed {
        step: String,
        agent: String,
        reason: String,
    },
}

/// Result of running a pipeline
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub pipeline_name: String,
    pub steps_passed: Vec<String>,
    pub steps_failed: Vec<String>,
    pub step_results: std::collections::HashMap<String, crate::context::StepResult>,
    pub audit_log: crate::audit::AuditLog,
    pub success: bool,
}

/// An event emitted to an output sink during pipeline execution
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// A chunk of LLM output (for streaming)
    LlmChunk { step: String, delta: String },
    /// A tool produced a chunk of output
    ToolChunk {
        step: String,
        tool: String,
        delta: String,
    },
    /// A step completed
    StepCompleted { step: String, output: crate::action::StepOutput },
    /// The pipeline completed
    PipelineCompleted { result: PipelineResult },
}

/// Trait for receiving pipeline output events (streaming support)
#[async_trait::async_trait]
pub trait OutputSink: Send + Sync {
    /// Emit an output event. Fire-and-forget — caller does not await completion.
    async fn emit(&self, event: OutputEvent);
}
