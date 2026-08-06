use super::GuardError;
use crate::context::StepContext;

pub fn check_only_allowed_agents_used(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    Ok(())
}

pub fn check_no_recursive_delegation(
    _guard: &super::Guard,
    _ctx: &StepContext,
) -> Result<(), GuardError> {
    Ok(())
}

pub fn check_delegated_agent_passed(
    _guard: &super::Guard,
    _ctx: &StepContext,
    _agent_name: &str,
) -> Result<(), GuardError> {
    Ok(())
}

/// Phase D3: Check if a detached agent has completed (by name)
pub fn check_detached_agent_completed(
    agent_name: &str,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Check if DelegationCompleted event exists for this agent in audit log
    for entry in &ctx.trace.entries {
        if entry.step_name == agent_name && entry.status == "completed" {
            return Ok(());
        }
    }
    Err(GuardError::Failed {
        guard: format!("DetachedAgentCompleted({})", agent_name),
        reason: format!("Detached agent '{}' has not yet completed", agent_name),
    })
}
