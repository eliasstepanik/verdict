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
                    AuditEvent::SelfUpdateProposed { agent_name, risk_level } => {
                        json!({ "type": "SelfUpdateProposed", "agent_name": agent_name, "risk_level": risk_level })
                    }
                    AuditEvent::AgentVersionCreated { agent_name, version } => {
                        json!({ "type": "AgentVersionCreated", "agent_name": agent_name, "version": version })
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
                            "SelfUpdateProposed" => AuditEvent::SelfUpdateProposed {
                                agent_name: event_obj
                                    .get("agent_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                risk_level: event_obj
                                    .get("risk_level")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                            },
                            "AgentVersionCreated" => AuditEvent::AgentVersionCreated {
                                agent_name: event_obj
                                    .get("agent_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
                                version: event_obj
                                    .get("version")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string(),
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

/// Monitoring server for Web UI dashboard (Phase 9)
pub struct MonitoringServer {
    audit_log: std::sync::Arc<std::sync::Mutex<AuditLog>>,
    trace: std::sync::Arc<std::sync::Mutex<crate::context::PipelineTrace>>,
}

impl MonitoringServer {
    /// Create a new monitoring server
    pub fn new(audit_log: AuditLog, trace: crate::context::PipelineTrace) -> Self {
        Self {
            audit_log: std::sync::Arc::new(std::sync::Mutex::new(audit_log)),
            trace: std::sync::Arc::new(std::sync::Mutex::new(trace)),
        }
    }

    /// Start the monitoring HTTP server on the given address
    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{
            routing::get,
            extract::State,
            Json,
            Router,
            response::{Html as AxumHtml, IntoResponse},
        };

        let audit_log = self.audit_log.clone();
        let trace = self.trace.clone();

        // App state structure
        #[derive(Clone)]
        struct AppState {
            audit_log: std::sync::Arc<std::sync::Mutex<AuditLog>>,
            trace: std::sync::Arc<std::sync::Mutex<crate::context::PipelineTrace>>,
        }

        let app_state = AppState {
            audit_log: audit_log.clone(),
            trace: trace.clone(),
        };

        // Handlers
        async fn index_handler() -> impl IntoResponse {
            let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Verdict Monitoring Dashboard</title>
    <style>
        body { font-family: monospace; margin: 20px; background: #f5f5f5; }
        h1 { color: #333; }
        .section { background: white; padding: 20px; margin: 10px 0; border-radius: 5px; }
        .entry { padding: 10px; border-left: 3px solid #0066cc; margin: 5px 0; }
        .error { border-left-color: #cc0000; }
        .success { border-left-color: #00cc00; }
    </style>
</head>
<body>
    <h1>Verdict Monitoring Dashboard</h1>
    <div class="section">
        <h2>Recent Audit Entries</h2>
        <p><a href="/api/entries">View all entries (JSON)</a></p>
    </div>
    <div class="section">
        <h2>Pipeline Trace</h2>
        <p><a href="/api/trace">View trace (JSON)</a></p>
    </div>
</body>
</html>
            "#;
            AxumHtml(html)
        }

        async fn entries_handler(
            State(state): State<AppState>,
        ) -> Json<Vec<AuditEntry>> {
            let log = state.audit_log.lock().ok();
            let entries: Vec<_> = log
                .map(|l| {
                    l.entries()
                        .iter()
                        .rev()
                        .take(100)
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            Json(entries)
        }

        async fn trace_handler(
            State(state): State<AppState>,
        ) -> Json<serde_json::Value> {
            let t = state.trace.lock().ok();
            let entries: Vec<_> = t
                .map(|tr| tr.entries.clone())
                .unwrap_or_default();
            Json(json!({ "entries": entries }))
        }

        let app = Router::new()
            .route("/", get(index_handler))
            .route("/api/entries", get(entries_handler))
            .route("/api/trace", get(trace_handler))
            .with_state(app_state);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}
