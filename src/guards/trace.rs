use super::GuardError;
use crate::context::StepContext;

pub fn check_trace_available(_guard: &super::Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if ctx.trace.entries.is_empty() {
        Err(GuardError::Failed {
            guard: "TraceAvailable".to_string(),
            reason: "no trace entries found".to_string(),
        })
    } else {
        Ok(())
    }
}

pub fn check_audit_log_written(_guard: &super::Guard, ctx: &StepContext) -> Result<(), GuardError> {
    if ctx.trace.entries.is_empty() {
        Err(GuardError::Failed {
            guard: "AuditLogWritten".into(),
            reason: "No trace entries recorded — audit log appears empty".into(),
        })
    } else {
        Ok(())
    }
}
