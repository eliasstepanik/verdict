use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Events that can be logged in an audit trail
#[derive(Debug, Clone)]
pub enum AuditEvent {
    StepStarted,
    GuardPassed { guard: String },
    GuardFailed { guard: String, reason: String },
    VerdictPassed { verdict: String },
    VerdictFailed { verdict: String, reason: String },
    StepCompleted { verdict_passed: bool },
    StepFailed { error: String },
    ToolCallStarted { tool: String, args: String },
    ToolCallCompleted { tool: String, output_bytes: usize },
    ToolCallFailed { tool: String, reason: String },
    PipelineStarted,
    PipelineCompleted { steps_passed: u32, steps_failed: u32 },
    PipelineFailed { reason: String },
}

/// A single audit log entry
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub pipeline_name: String,
    pub step_name: String,
    pub event: AuditEvent,
}

/// Audit log for pipeline execution
#[derive(Debug, Clone)]
pub struct AuditLog {
    entries: Vec<AuditEntry>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn append(&mut self, entry: AuditEntry) {
        self.entries.push(entry);
    }

    pub fn entries(&self) -> &[AuditEntry] {
        &self.entries
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let entries: Vec<Value> = self
            .entries
            .iter()
            .map(|entry| {
                let event_json = match &entry.event {
                    AuditEvent::StepStarted => json!({ "type": "StepStarted" }),
                    AuditEvent::GuardPassed { guard } => {
                        json!({ "type": "GuardPassed", "guard": guard })
                    }
                    AuditEvent::GuardFailed { guard, reason } => {
                        json!({ "type": "GuardFailed", "guard": guard, "reason": reason })
                    }
                    AuditEvent::VerdictPassed { verdict } => {
                        json!({ "type": "VerdictPassed", "verdict": verdict })
                    }
                    AuditEvent::VerdictFailed { verdict, reason } => {
                        json!({ "type": "VerdictFailed", "verdict": verdict, "reason": reason })
                    }
                    AuditEvent::StepCompleted { verdict_passed } => {
                        json!({ "type": "StepCompleted", "verdict_passed": verdict_passed })
                    }
                    AuditEvent::StepFailed { error } => {
                        json!({ "type": "StepFailed", "error": error })
                    }
                    AuditEvent::ToolCallStarted { tool, args } => {
                        json!({ "type": "ToolCallStarted", "tool": tool, "args": args })
                    }
                    AuditEvent::ToolCallCompleted {
                        tool,
                        output_bytes,
                    } => {
                        json!({ "type": "ToolCallCompleted", "tool": tool, "output_bytes": output_bytes })
                    }
                    AuditEvent::ToolCallFailed { tool, reason } => {
                        json!({ "type": "ToolCallFailed", "tool": tool, "reason": reason })
                    }
                    AuditEvent::PipelineStarted => json!({ "type": "PipelineStarted" }),
                    AuditEvent::PipelineCompleted {
                        steps_passed,
                        steps_failed,
                    } => {
                        json!({ "type": "PipelineCompleted", "steps_passed": steps_passed, "steps_failed": steps_failed })
                    }
                    AuditEvent::PipelineFailed { reason } => {
                        json!({ "type": "PipelineFailed", "reason": reason })
                    }
                };

                json!({
                    "timestamp": entry.timestamp.to_rfc3339(),
                    "pipeline_name": entry.pipeline_name,
                    "step_name": entry.step_name,
                    "event": event_json,
                })
            })
            .collect();

        let log_json = json!({
            "entries": entries
        });

        serde_json::to_string(&log_json)
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}
