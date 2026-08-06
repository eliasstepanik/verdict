//! Telemetry export for verdict-app
//!
//! Converts audit logs to OpenTelemetry spans and exports them.

use verdict::audit::AuditLog;
use verdict_telemetry::{audit_log_to_spans, OtelExporter, StdoutExporter};

/// Export audit log as OpenTelemetry spans via stdout
pub async fn export_telemetry(audit_log: &AuditLog) {
    let spans = audit_log_to_spans(audit_log);
    let exporter = StdoutExporter;
    exporter.export(spans).await;
}
