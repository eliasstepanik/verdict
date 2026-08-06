use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// OpenTelemetry span representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelSpan {
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time: DateTime<Utc>,
    pub end_time: Option<DateTime<Utc>>,
    pub attributes: HashMap<String, String>,
    pub status: OtelStatus,
    pub events: Vec<OtelEvent>,
}

/// OpenTelemetry event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelEvent {
    pub name: String,
    pub timestamp: DateTime<Utc>,
    pub attributes: HashMap<String, String>,
}

/// OpenTelemetry span status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OtelStatus {
    Unset,
    Ok,
    Error { message: String },
}

/// Trait for exporting OpenTelemetry spans
#[async_trait]
pub trait OtelExporter: Send + Sync {
    async fn export(&self, spans: Vec<OtelSpan>);
}

/// Stdout exporter that prints spans as JSON
pub struct StdoutExporter;

#[async_trait]
impl OtelExporter for StdoutExporter {
    async fn export(&self, spans: Vec<OtelSpan>) {
        for span in spans {
            if let Ok(json_str) = serde_json::to_string_pretty(&span) {
                println!("{}", json_str);
            }
        }
    }
}

/// Jaeger exporter (feature-gated: `jaeger`)
#[cfg(feature = "jaeger")]
pub struct JaegerExporter {
    pub endpoint: String,
}

#[cfg(feature = "jaeger")]
#[async_trait]
impl OtelExporter for JaegerExporter {
    async fn export(&self, spans: Vec<OtelSpan>) {
        let _ = reqwest::Client::new()
            .post(&format!("{}/api/traces", self.endpoint))
            .json(&serde_json::json!({
                "resourceSpans": [
                    {
                        "scopeSpans": [
                            {
                                "spans": spans
                            }
                        ]
                    }
                ]
            }))
            .send()
            .await;
    }
}

/// OTLP exporter (feature-gated: `otlp`)
#[cfg(feature = "otlp")]
pub struct OtlpExporter {
    pub endpoint: String,
}

#[cfg(feature = "otlp")]
#[async_trait]
impl OtelExporter for OtlpExporter {
    async fn export(&self, spans: Vec<OtelSpan>) {
        let _ = reqwest::Client::new()
            .post(&format!("{}/v1/traces", self.endpoint))
            .json(&serde_json::json!({
                "resourceSpans": [
                    {
                        "scopeSpans": [
                            {
                                "spans": spans
                            }
                        ]
                    }
                ]
            }))
            .send()
            .await;
    }
}

/// Convert audit log entries to OpenTelemetry spans
pub fn audit_log_to_spans(audit_log: &verdict::audit::AuditLog) -> Vec<OtelSpan> {
    use verdict::audit::AuditEvent;

    let mut spans: Vec<OtelSpan> = Vec::new();
    let mut span_map: HashMap<String, OtelSpan> = HashMap::new();
    let mut trace_id = String::from("default-trace");
    let mut next_span_id = 1u32;

    for entry in audit_log.entries() {
        match &entry.event {
            AuditEvent::PipelineStarted => {
                trace_id = format!("trace-{}", chrono::Utc::now().timestamp_millis());
            }
            AuditEvent::StepStarted => {
                let span_id = format!("span-{}", next_span_id);
                next_span_id += 1;

                let span = OtelSpan {
                    trace_id: trace_id.clone(),
                    span_id: span_id.clone(),
                    parent_span_id: None,
                    name: format!("{}.{}", entry.pipeline_name, entry.step_name),
                    start_time: entry.timestamp,
                    end_time: None,
                    attributes: {
                        let mut m = HashMap::new();
                        m.insert("pipeline".to_string(), entry.pipeline_name.clone());
                        m.insert("step".to_string(), entry.step_name.clone());
                        m
                    },
                    status: OtelStatus::Unset,
                    events: Vec::new(),
                };

                span_map.insert(format!("{}.{}", entry.pipeline_name, entry.step_name), span);
            }
            AuditEvent::StepCompleted { .. } => {
                let key = format!("{}.{}", entry.pipeline_name, entry.step_name);
                if let Some(span) = span_map.get_mut(&key) {
                    span.end_time = Some(entry.timestamp);
                    span.status = OtelStatus::Ok;
                    spans.push(span.clone());
                    span_map.remove(&key);
                }
            }
            AuditEvent::DelegationStarted {
                parent_agent,
                child_agent,
                depth,
            } => {
                let span_id = format!("span-{}", next_span_id);
                next_span_id += 1;

                let span = OtelSpan {
                    trace_id: trace_id.clone(),
                    span_id: span_id.clone(),
                    parent_span_id: None,
                    name: format!("delegation:{}->{}", parent_agent, child_agent),
                    start_time: entry.timestamp,
                    end_time: None,
                    attributes: {
                        let mut m = HashMap::new();
                        m.insert("parent_agent".to_string(), parent_agent.clone());
                        m.insert("child_agent".to_string(), child_agent.clone());
                        m.insert("depth".to_string(), depth.to_string());
                        m
                    },
                    status: OtelStatus::Unset,
                    events: Vec::new(),
                };

                span_map.insert(
                    format!("delegation:{}->{}", parent_agent, child_agent),
                    span,
                );
            }
            AuditEvent::DelegationCompleted {
                parent_agent,
                child_agent,
                depth: _,
            } => {
                let key = format!("delegation:{}->{}", parent_agent, child_agent);
                if let Some(span) = span_map.get_mut(&key) {
                    span.end_time = Some(entry.timestamp);
                    span.status = OtelStatus::Ok;
                    spans.push(span.clone());
                    span_map.remove(&key);
                }
            }
            AuditEvent::DelegationFailed {
                parent_agent,
                child_agent,
                depth: _,
                reason,
            } => {
                let key = format!("delegation:{}->{}", parent_agent, child_agent);
                if let Some(span) = span_map.get_mut(&key) {
                    span.end_time = Some(entry.timestamp);
                    span.status = OtelStatus::Error {
                        message: reason.clone(),
                    };
                    spans.push(span.clone());
                    span_map.remove(&key);
                }
            }
            AuditEvent::ToolCallStarted { tool, args: _ } => {
                let event = OtelEvent {
                    name: format!("tool_call:{}", tool),
                    timestamp: entry.timestamp,
                    attributes: {
                        let mut m = HashMap::new();
                        m.insert("tool".to_string(), tool.clone());
                        m
                    },
                };

                let key = format!("{}.{}", entry.pipeline_name, entry.step_name);
                if let Some(span) = span_map.get_mut(&key) {
                    span.events.push(event);
                }
            }
            _ => {}
        }
    }

    spans
}
