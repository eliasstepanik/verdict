use serde_json::Value;

use super::types::AuditEvent;

/// Reconstruct an AuditEvent from a JSON object
pub fn event_from_json(event_obj: &serde_json::Map<String, Value>) -> Option<AuditEvent> {
    let event_type = event_obj.get("type")?.as_str()?;
    match event_type {
        "StepStarted" => Some(AuditEvent::StepStarted),
        "GuardPassed" => Some(AuditEvent::GuardPassed {
            guard: event_obj
                .get("guard")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        }),
        "GuardFailed" => Some(AuditEvent::GuardFailed {
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
        }),
        "VerdictPassed" => Some(AuditEvent::VerdictPassed {
            verdict: event_obj
                .get("verdict")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string(),
        }),
        "VerdictFailed" => Some(AuditEvent::VerdictFailed {
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
        }),
        "StepCompleted" => Some(AuditEvent::StepCompleted {
            verdict_passed: event_obj
                .get("verdict_passed")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        }),
        "StepFailed" => Some(AuditEvent::StepFailed {
            error: event_obj
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "ToolCallStarted" => Some(AuditEvent::ToolCallStarted {
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
        }),
        "ToolCallCompleted" => Some(AuditEvent::ToolCallCompleted {
            tool: event_obj
                .get("tool")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            output_bytes: event_obj
                .get("output_bytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                as usize,
        }),
        "ToolCallFailed" => Some(AuditEvent::ToolCallFailed {
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
        }),
        "PipelineStarted" => Some(AuditEvent::PipelineStarted),
        "PipelineCompleted" => Some(AuditEvent::PipelineCompleted {
            steps_passed: event_obj
                .get("steps_passed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                as u32,
            steps_failed: event_obj
                .get("steps_failed")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                as u32,
        }),
        "PipelineFailed" => Some(AuditEvent::PipelineFailed {
            reason: event_obj
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "DelegationStarted" => Some(AuditEvent::DelegationStarted {
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
            depth: event_obj.get("depth").and_then(|v| v.as_u64()).unwrap_or(0)
                as u32,
        }),
        "DelegationCompleted" => Some(AuditEvent::DelegationCompleted {
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
            depth: event_obj.get("depth").and_then(|v| v.as_u64()).unwrap_or(0)
                as u32,
        }),
        "DelegationFailed" => Some(AuditEvent::DelegationFailed {
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
            depth: event_obj.get("depth").and_then(|v| v.as_u64()).unwrap_or(0)
                as u32,
            reason: event_obj
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "InjectionDetected" => Some(AuditEvent::InjectionDetected {
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
        }),
        "SecretDetected" => Some(AuditEvent::SecretDetected {
            pattern_name: event_obj
                .get("pattern_name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "BudgetExceeded" => Some(AuditEvent::BudgetExceeded {
            reason: event_obj
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        "RateLimitHit" => Some(AuditEvent::RateLimitHit {
            calls_this_minute: event_obj
                .get("calls_this_minute")
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                as u32,
        }),
        "SelfUpdateProposed" => Some(AuditEvent::SelfUpdateProposed {
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
        }),
        "AgentVersionCreated" => Some(AuditEvent::AgentVersionCreated {
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
        }),
        "FallbackTriggered" => Some(AuditEvent::FallbackTriggered {
            step: event_obj
                .get("step")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            reason: event_obj
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        _ => None,
    }
}
