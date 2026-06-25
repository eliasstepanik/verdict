use crate::action::StepError;
use crate::guards::{GuardError, GuardPhase};
use crate::verdict::VerdictError;
use thiserror::Error;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

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

/// State of a suspended pipeline (Phase D4)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuspendedState {
    /// Token used to identify and resume this suspension
    pub state_token: String,
    /// Name of the step that suspended execution
    pub step_name: String,
    /// Reason for suspension
    pub reason: String,
    /// When the pipeline was suspended
    pub suspended_at: DateTime<Utc>,
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
    /// Total cost in USD (A6)
    pub total_cost_usd: f64,
    /// Total tokens used (A6)
    pub total_tokens_used: u32,
    /// Structured log entries (A7)
    pub log: Vec<LogEntry>,
    /// Suspended state, if pipeline was suspended (Phase D4)
    pub suspended: Option<SuspendedState>,
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
    /// Tool approval required before execution (A1)
    ToolApprovalRequired {
        step: String,
        tool: String,
        args: serde_json::Value,
    },
    /// A log entry (A7)
    Log(LogEntry),
}

/// Log level for structured logging (A7)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

/// Structured log entry with trace correlation (A7)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: LogLevel,
    pub pipeline: String,
    pub step: String,
    pub trace_id: String,
    pub span_id: String,
    pub message: String,
    pub fields: serde_json::Value,
}

/// Trait for receiving pipeline output events (streaming support)
#[async_trait::async_trait]
pub trait OutputSink: Send + Sync {
    /// Emit an output event. Fire-and-forget — caller does not await completion.
    async fn emit(&self, event: OutputEvent);
}
