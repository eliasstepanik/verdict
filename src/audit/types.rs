use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a node in the agent delegation call tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallTreeNode {
    pub agent_name: String,
    pub depth: u32,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub status: CallTreeStatus,
    pub children: Vec<CallTreeNode>,
}

/// Status of a call tree node
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CallTreeStatus {
    Running,
    Completed,
    Failed { reason: String },
}

/// Events that can be logged in an audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuditEvent {
    StepStarted,
    GuardPassed {
        guard: String,
    },
    GuardFailed {
        guard: String,
        reason: String,
    },
    VerdictPassed {
        verdict: String,
    },
    VerdictFailed {
        verdict: String,
        reason: String,
    },
    StepCompleted {
        verdict_passed: bool,
    },
    StepFailed {
        error: String,
    },
    ToolCallStarted {
        tool: String,
        args: String,
    },
    ToolCallCompleted {
        tool: String,
        output_bytes: usize,
    },
    ToolCallFailed {
        tool: String,
        reason: String,
    },
    PipelineStarted,
    PipelineCompleted {
        steps_passed: u32,
        steps_failed: u32,
    },
    PipelineFailed {
        reason: String,
    },
    /// Delegation to a child agent started
    DelegationStarted {
        parent_agent: String,
        child_agent: String,
        depth: u32,
    },
    /// Delegation completed successfully
    DelegationCompleted {
        parent_agent: String,
        child_agent: String,
        depth: u32,
    },
    /// Delegation failed
    DelegationFailed {
        parent_agent: String,
        child_agent: String,
        depth: u32,
        reason: String,
    },
    /// Injection pattern detected
    InjectionDetected {
        pattern: String,
        risk_level: String,
    },
    /// Secret pattern detected
    SecretDetected {
        pattern_name: String,
    },
    /// Budget exceeded
    BudgetExceeded {
        reason: String,
    },
    /// Rate limit hit
    RateLimitHit {
        calls_this_minute: u32,
    },
    /// Self-update proposal validated
    SelfUpdateProposed {
        agent_name: String,
        risk_level: String,
    },
    /// Agent version created
    AgentVersionCreated {
        agent_name: String,
        version: String,
    },
    /// Fallback pipeline triggered
    FallbackTriggered {
        step: String,
        reason: String,
    },
    /// Tool approval requested (A1)
    ToolApprovalRequested {
        tool: String,
    },
    /// Tool approval granted (A1)
    ToolApprovalGranted {
        tool: String,
    },
    /// Tool approval denied (A1)
    ToolApprovalDenied {
        tool: String,
    },
}

/// A single audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub pipeline_name: String,
    pub step_name: String,
    pub event: AuditEvent,
}
