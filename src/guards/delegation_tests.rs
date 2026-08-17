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
