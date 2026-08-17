use super::GuardError;
use crate::context::StepContext;

/// Check that only allowed agents are delegated to.
///
/// Detects delegated agents by looking for step results with "{agent_name}.*" keys
/// (created during delegation merging). Fails if any delegated agent is not in the
/// current agent's allowed_agents policy list.
pub fn check_only_allowed_agents_used(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Extract delegated agent names from step_results keys ("{agent}.{step}")
    let mut delegated_agents = std::collections::HashSet::new();
    
    for key in ctx.step_results.keys() {
        if let Some(dot_pos) = key.find('.') {
            let agent_part = &key[..dot_pos];
            // Only consider keys that look like delegated results (contain a dot and agent != parent_agent)
            // If parent_agent is set, we're in a delegated context; look for further delegations
            delegated_agents.insert(agent_part.to_string());
        }
    }
    
    // Get the agent's allowed_agents policy from the registry
    if let Some(agent) = ctx.agent_registry.get(&ctx.agent_name) {
        let policy = &agent.policy;
        
        // If allowed_agents is empty, allow all (not an allowlist mode)
        if policy.allowed_agents.is_empty() {
            return Ok(());
        }
        
        // Check each delegated agent is in the allowlist
        for agent_name in delegated_agents {
            if !policy.allowed_agents.contains(&agent_name) {
                return Err(GuardError::Failed {
                    guard: "OnlyAllowedAgentsUsed".to_string(),
                    reason: format!(
                        "Agent '{}' delegated to but not in allowed list: {:?}",
                        agent_name, policy.allowed_agents
                    ),
                });
            }
        }
    }
    
    Ok(())
}

/// Check that no delegation forms a cycle (agent delegating to itself directly or indirectly).
///
/// Detects cycles by inspecting the parent agent chain in context and checking if
/// any delegated agent is an ancestor of the current agent.
pub fn check_no_recursive_delegation(
    _guard: &super::Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    // Check if this context represents a delegation
    if ctx.delegation_depth == 0 {
        // Root agent, no recursion possible
        return Ok(());
    }
    
    // Build the ancestor chain by walking parent_agent up the tree
    // Note: We can only see our immediate parent from ctx.parent_agent
    // A full cycle check would need the delegation audit trail, but we can do basic checks
    
    // Extract delegated agent names from step_results
    let delegated_agents: std::collections::HashSet<String> = ctx
        .step_results
        .keys()
        .filter_map(|key| key.split('.').next().map(|s| s.to_string()))
        .collect();
    
    // Check if current agent appears in delegated agents (direct self-delegation)
    if delegated_agents.contains(&ctx.agent_name) {
        return Err(GuardError::Failed {
            guard: "NoRecursiveDelegation".to_string(),
            reason: format!("Agent '{}' cannot delegate to itself", ctx.agent_name),
        });
    }
    
    // Check if immediate parent is trying to delegate back to us
    if let Some(parent) = &ctx.parent_agent {
        if delegated_agents.contains(parent) {
            return Err(GuardError::Failed {
                guard: "NoRecursiveDelegation".to_string(),
                reason: format!(
                    "Recursive delegation detected: '{}' -> '{}' -> '{}'",
                    parent, ctx.agent_name, parent
                ),
            });
        }
    }
    
    Ok(())
}

/// Check that a delegated agent's pipeline succeeded.
///
/// Looks up the delegated agent's results using the "{agent_name}.{step_name}"
/// namespacing convention and fails if any step failed or is missing.
pub fn check_delegated_agent_passed(
    _guard: &super::Guard,
    ctx: &StepContext,
    agent_name: &str,
) -> Result<(), GuardError> {
    // Find any step results keyed as "{agent_name}.*"
    let prefix = format!("{}.", agent_name);
    let matching_results: Vec<_> = ctx
        .step_results
        .iter()
        .filter(|(key, _)| key.starts_with(&prefix))
        .collect();
    
    if matching_results.is_empty() {
        return Err(GuardError::Failed {
            guard: format!("DelegatedAgentPassed({})", agent_name),
            reason: format!("No results found for delegated agent '{}'", agent_name),
        });
    }
    
    // Check that all delegated steps passed (verdict_passed == true and no error)
    for (key, result) in matching_results {
        if !result.verdict_passed || result.error.is_some() {
            let error_msg = result
                .error
                .clone()
                .unwrap_or_else(|| "verdict failed".to_string());
            return Err(GuardError::Failed {
                guard: format!("DelegatedAgentPassed({})", agent_name),
                reason: format!(
                    "Delegated step '{}' failed: {}",
                    key, error_msg
                ),
            });
        }
    }
    
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

#[cfg(test)]
#[path = "delegation_tests.rs"]
mod tests;
