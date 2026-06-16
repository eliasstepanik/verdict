use crate::context::StepContext;
use super::GuardError;

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
