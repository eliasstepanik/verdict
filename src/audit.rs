use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use serde::{Deserialize, Serialize};

/// Events that can be logged in an audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

/// A single audit log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: DateTime<Utc>,
    pub pipeline_name: String,
    pub step_name: String,
    pub event: AuditEvent,
}

/// Audit log for pipeline execution
#[derive(Debug, Clone, Serialize, Deserialize)]
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
                    AuditEvent::DelegationStarted { parent_agent, child_agent, depth } => {
                        json!({ "type": "DelegationStarted", "parent_agent": parent_agent, "child_agent": child_agent, "depth": depth })
                    }
                    AuditEvent::DelegationCompleted { parent_agent, child_agent, depth } => {
                        json!({ "type": "DelegationCompleted", "parent_agent": parent_agent, "child_agent": child_agent, "depth": depth })
                    }
                    AuditEvent::DelegationFailed { parent_agent, child_agent, depth, reason } => {
                        json!({ "type": "DelegationFailed", "parent_agent": parent_agent, "child_agent": child_agent, "depth": depth, "reason": reason })
                    }
                    AuditEvent::InjectionDetected { pattern, risk_level } => {
                        json!({ "type": "InjectionDetected", "pattern": pattern, "risk_level": risk_level })
                    }
                    AuditEvent::SecretDetected { pattern_name } => {
                        json!({ "type": "SecretDetected", "pattern_name": pattern_name })
                    }
                    AuditEvent::BudgetExceeded { reason } => {
                        json!({ "type": "BudgetExceeded", "reason": reason })
                    }
                    AuditEvent::RateLimitHit { calls_this_minute } => {
                        json!({ "type": "RateLimitHit", "calls_this_minute": calls_this_minute })
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

    /// Save audit log to a file as JSON
    pub fn save_to_file(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let json_str = self.to_json()?;
        std::fs::write(path, json_str)?;
        Ok(())
    }

    /// Load audit log from a JSON file
    pub fn load_from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let json_str = std::fs::read_to_string(path)?;
        let log_json: Value = serde_json::from_str(&json_str)?;

        let mut entries = Vec::new();

        if let Some(entries_array) = log_json.get("entries").and_then(|v| v.as_array()) {
            for entry_json in entries_array {
                if let (
                    Some(timestamp_str),
                    Some(pipeline_name),
                    Some(step_name),
                    Some(event_obj),
                ) = (
                    entry_json.get("timestamp").and_then(|v| v.as_str()),
                    entry_json.get("pipeline_name").and_then(|v| v.as_str()),
                    entry_json.get("step_name").and_then(|v| v.as_str()),
                    entry_json.get("event").and_then(|v| v.as_object()),
                ) {
                    let timestamp = DateTime::parse_from_rfc3339(timestamp_str)?
                        .with_timezone(&Utc);

                    // Reconstruct AuditEvent from JSON object
                    let event = if let Some(event_type) = event_obj.get("type").and_then(|v| v.as_str())
                    {
                        match event_type {
                            "StepStarted" => AuditEvent::StepStarted,
                            "GuardPassed" => AuditEvent::GuardPassed {
                                guard: event_obj
                                    .get("guard")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                            },
                            "GuardFailed" => AuditEvent::GuardFailed {
                                guard: event_obj
                                    .get("guard")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                reason: event_obj
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "VerdictPassed" => AuditEvent::VerdictPassed {
                                verdict: event_obj
                                    .get("verdict")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                            },
                            "VerdictFailed" => AuditEvent::VerdictFailed {
                                verdict: event_obj
                                    .get("verdict")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                reason: event_obj
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "StepCompleted" => AuditEvent::StepCompleted {
                                verdict_passed: event_obj
                                    .get("verdict_passed")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false),
                            },
                            "StepFailed" => AuditEvent::StepFailed {
                                error: event_obj
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "ToolCallStarted" => AuditEvent::ToolCallStarted {
                                tool: event_obj
                                    .get("tool")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                args: event_obj
                                    .get("args")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "ToolCallCompleted" => AuditEvent::ToolCallCompleted {
                                tool: event_obj
                                    .get("tool")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                output_bytes: event_obj
                                    .get("output_bytes")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as usize,
                            },
                            "ToolCallFailed" => AuditEvent::ToolCallFailed {
                                tool: event_obj
                                    .get("tool")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                reason: event_obj
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "PipelineStarted" => AuditEvent::PipelineStarted,
                            "PipelineCompleted" => AuditEvent::PipelineCompleted {
                                steps_passed: event_obj
                                    .get("steps_passed")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32,
                                steps_failed: event_obj
                                    .get("steps_failed")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32,
                            },
                            "PipelineFailed" => AuditEvent::PipelineFailed {
                                reason: event_obj
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "DelegationStarted" => AuditEvent::DelegationStarted {
                                parent_agent: event_obj
                                    .get("parent_agent")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                child_agent: event_obj
                                    .get("child_agent")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                depth: event_obj
                                    .get("depth")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32,
                            },
                            "DelegationCompleted" => AuditEvent::DelegationCompleted {
                                parent_agent: event_obj
                                    .get("parent_agent")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                child_agent: event_obj
                                    .get("child_agent")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                depth: event_obj
                                    .get("depth")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32,
                            },
                            "DelegationFailed" => AuditEvent::DelegationFailed {
                                parent_agent: event_obj
                                    .get("parent_agent")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                child_agent: event_obj
                                    .get("child_agent")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                depth: event_obj
                                    .get("depth")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32,
                                reason: event_obj
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "InjectionDetected" => AuditEvent::InjectionDetected {
                                pattern: event_obj
                                    .get("pattern")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                risk_level: event_obj
                                    .get("risk_level")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "SecretDetected" => AuditEvent::SecretDetected {
                                pattern_name: event_obj
                                    .get("pattern_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "BudgetExceeded" => AuditEvent::BudgetExceeded {
                                reason: event_obj
                                    .get("reason")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "RateLimitHit" => AuditEvent::RateLimitHit {
                                calls_this_minute: event_obj
                                    .get("calls_this_minute")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0) as u32,
                            },
                            _ => continue,
                        }
                    } else {
                        continue;
                    };

                    entries.push(AuditEntry {
                        timestamp,
                        pipeline_name: pipeline_name.to_string(),
                        step_name: step_name.to_string(),
                        event,
                    });
                }
            }
        }

        Ok(AuditLog { entries })
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}
