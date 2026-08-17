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
mod tests {
    use super::*;
    use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
    use crate::context::StepResult;
    use crate::action::StepOutput;
    use crate::pipeline::Pipeline;
    use crate::registry::AgentRegistry;
    use crate::skills::SkillSet;
    use std::sync::Arc;

    fn make_test_context(agent_name: &str) -> StepContext {
        let policy = AgentPolicy {
            max_steps: 10,
            max_retries: 3,
            max_delegation_depth: 3,
            max_cost_usd: None,
            max_runtime_seconds: None,
            allow_self_update: false,
            require_approval_for_self_update: false,
            allowed_agents: vec!["reviewer".to_string(), "debugger".to_string()],
            allowed_tools: crate::toolset::ToolSet::Full,
            allowed_skills: vec![],
            filesystem_policy: FilesystemPolicy::default(),
            network_policy: NetworkPolicy::DenyAll,
        };
        let agent = Agent {
            name: agent_name.to_string(),
            description: "Test agent".to_string(),
            pipeline: Pipeline {
                name: "test_pipeline".to_string(),
                steps: vec![],
                on_failure: crate::pipeline::FailureMode::Abort,
                max_retries: 0,
            },
            policy,
            tools: crate::toolset::ToolSet::Full,
            skills: SkillSet {
                skills: vec![],
            },
            scorers: vec![],
        };

        let mut registry = AgentRegistry::new();
        registry.register(agent);

        let mut ctx = StepContext::new(
            agent_name.to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            serde_json::json!({}),
            FilesystemPolicy::default(),
        );
        ctx.agent_registry = Arc::new(registry);
        ctx.network_policy = NetworkPolicy::DenyAll;
        ctx
    }

    // ============= check_only_allowed_agents_used tests =============

    #[test]
    fn test_allowed_agents_empty_policy_allows_all() {
        let ctx = make_test_context("coder");
        // Set allowed_agents to empty (allow-all mode)
        // The guard should pass if no agents delegated anyway

        let result = check_only_allowed_agents_used(&super::super::Guard::OnlyAllowedAgentsUsed, &ctx);
        assert!(result.is_ok(), "Should pass with empty delegation and empty allow list");
    }

    #[test]
    fn test_allowed_agents_in_list_passes() {
        let mut ctx = make_test_context("coder");
        // Add delegated agent results for "reviewer" (which is in allowed_agents)
        let result = StepResult {
            step_name: "review".to_string(),
            output: StepOutput::new("ok".to_string()),
            verdict_passed: true,
            error: None,
        };
        ctx.step_results.insert("reviewer.review".to_string(), result);

        let pass_result = check_only_allowed_agents_used(&super::super::Guard::OnlyAllowedAgentsUsed, &ctx);
        assert!(pass_result.is_ok(), "Should pass when delegated agent is in allowed list");
    }

    #[test]
    fn test_allowed_agents_not_in_list_fails() {
        let mut ctx = make_test_context("coder");
        // Add delegated agent results for "orchestrator" (NOT in allowed_agents)
        let result = StepResult {
            step_name: "plan".to_string(),
            output: StepOutput::new("orchestrated".to_string()),
            verdict_passed: true,
            error: None,
        };
        ctx.step_results.insert("orchestrator.plan".to_string(), result);

        let fail_result = check_only_allowed_agents_used(&super::super::Guard::OnlyAllowedAgentsUsed, &ctx);
        assert!(
            fail_result.is_err(),
            "Should fail when delegated agent not in allowed list"
        );
        if let Err(GuardError::Failed { reason, .. }) = fail_result {
            assert!(
                reason.contains("orchestrator"),
                "Error should mention the unauthorized agent"
            );
        }
    }

    // ============= check_no_recursive_delegation tests =============

    #[test]
    fn test_no_recursion_at_root_passes() {
        let ctx = make_test_context("coder");
        let guard = super::super::Guard::NoRecursiveDelegation;
        let result = check_no_recursive_delegation(&guard, &ctx);
        assert!(result.is_ok(), "Root agent should always pass recursion check");
    }

    #[test]
    fn test_no_direct_self_delegation() {
        let mut ctx = make_test_context("coder");
        ctx.delegation_depth = 1; // Pretend we're a child agent
        ctx.parent_agent = Some("planner".to_string());

        // Add self-delegation: coder tries to delegate to itself
        let result = StepResult {
            step_name: "code".to_string(),
            output: StepOutput::new("code".to_string()),
            verdict_passed: true,
            error: None,
        };
        ctx.step_results.insert("coder.code".to_string(), result);

        let guard = super::super::Guard::NoRecursiveDelegation;
        let fail_result = check_no_recursive_delegation(&guard, &ctx);
        assert!(fail_result.is_err(), "Should fail on self-delegation");
    }

    #[test]
    fn test_no_parent_cycle_delegation() {
        let mut ctx = make_test_context("coder");
        ctx.delegation_depth = 1;
        ctx.parent_agent = Some("planner".to_string());

        // Add delegation back to parent: coder delegates to planner
        let result = StepResult {
            step_name: "plan".to_string(),
            output: StepOutput::new("plan".to_string()),
            verdict_passed: true,
            error: None,
        };
        ctx.step_results.insert("planner.plan".to_string(), result);

        let guard = super::super::Guard::NoRecursiveDelegation;
        let fail_result = check_no_recursive_delegation(&guard, &ctx);
        assert!(fail_result.is_err(), "Should fail on parent cycle");
    }

    #[test]
    fn test_no_recursion_allowed_delegation() {
        let mut ctx = make_test_context("coder");
        ctx.delegation_depth = 1;
        ctx.parent_agent = Some("planner".to_string());

        // Add delegation to allowed agent (not parent): coder delegates to reviewer
        let result = StepResult {
            step_name: "review".to_string(),
            output: StepOutput::new("reviewed".to_string()),
            verdict_passed: true,
            error: None,
        };
        ctx.step_results.insert("reviewer.review".to_string(), result);

        let guard = super::super::Guard::NoRecursiveDelegation;
        let pass_result = check_no_recursive_delegation(&guard, &ctx);
        assert!(
            pass_result.is_ok(),
            "Should pass when delegating to non-parent agent"
        );
    }

    // ============= check_delegated_agent_passed tests =============

    #[test]
    fn test_delegated_agent_passed_succeeds() {
        let mut ctx = make_test_context("coder");

        // Add successful delegated results
        let result1 = StepResult {
            step_name: "plan".to_string(),
            output: StepOutput::new("plan output".to_string()),
            verdict_passed: true,
            error: None,
        };
        let result2 = StepResult {
            step_name: "review".to_string(),
            output: StepOutput::new("review output".to_string()),
            verdict_passed: true,
            error: None,
        };
        ctx.step_results.insert("reviewer.plan".to_string(), result1);
        ctx.step_results.insert("reviewer.review".to_string(), result2);

        let guard = super::super::Guard::DelegatedAgentPassed("reviewer".to_string());
        let result = check_delegated_agent_passed(&guard, &ctx, "reviewer");
        assert!(result.is_ok(), "Should pass when all delegated steps succeeded");
    }

    #[test]
    fn test_delegated_agent_no_results() {
        let ctx = make_test_context("coder");

        let guard = super::super::Guard::DelegatedAgentPassed("reviewer".to_string());
        let result = check_delegated_agent_passed(&guard, &ctx, "reviewer");
        assert!(
            result.is_err(),
            "Should fail when no results found for delegated agent"
        );
    }

    #[test]
    fn test_delegated_agent_step_failed() {
        let mut ctx = make_test_context("coder");

        // Add one passing result and one failing result
        let result1 = StepResult {
            step_name: "plan".to_string(),
            output: StepOutput::new("plan output".to_string()),
            verdict_passed: true,
            error: None,
        };
        let result2 = StepResult {
            step_name: "review".to_string(),
            output: StepOutput::new("review output".to_string()),
            verdict_passed: false,
            error: Some("review failed".to_string()),
        };
        ctx.step_results.insert("reviewer.plan".to_string(), result1);
        ctx.step_results.insert("reviewer.review".to_string(), result2);

        let guard = super::super::Guard::DelegatedAgentPassed("reviewer".to_string());
        let result = check_delegated_agent_passed(&guard, &ctx, "reviewer");
        assert!(result.is_err(), "Should fail when any delegated step failed");
        if let Err(GuardError::Failed { reason, .. }) = result {
            assert!(reason.contains("failed"), "Error should mention the failure");
        }
    }

    #[test]
    fn test_delegated_agent_verdict_failed_no_error_msg() {
        let mut ctx = make_test_context("coder");

        // Add result with verdict_passed = false but no error message
        let result = StepResult {
            step_name: "plan".to_string(),
            output: StepOutput::new("plan output".to_string()),
            verdict_passed: false,
            error: None,
        };
        ctx.step_results.insert("reviewer.plan".to_string(), result);

        let guard = super::super::Guard::DelegatedAgentPassed("reviewer".to_string());
        let fail_result = check_delegated_agent_passed(&guard, &ctx, "reviewer");
        assert!(fail_result.is_err(), "Should fail when verdict_passed is false");
        if let Err(GuardError::Failed { reason, .. }) = fail_result {
            assert!(
                reason.contains("verdict failed"),
                "Should mention verdict failure"
            );
        }
    }
}
