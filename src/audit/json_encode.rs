use serde_json::{json, Value};

use super::types::AuditEvent;

/// Convert an AuditEvent to its JSON representation
pub fn event_to_json(event: &AuditEvent) -> Value {
    match event {
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
        AuditEvent::FallbackTriggered { step, reason } => {
            json!({ "type": "FallbackTriggered", "step": step, "reason": reason })
        }
        AuditEvent::ToolApprovalRequested { tool } => {
            json!({ "type": "ToolApprovalRequested", "tool": tool })
        }
        AuditEvent::ToolApprovalGranted { tool } => {
            json!({ "type": "ToolApprovalGranted", "tool": tool })
        }
        AuditEvent::ToolApprovalDenied { tool } => {
            json!({ "type": "ToolApprovalDenied", "tool": tool })
        }
    }
}
