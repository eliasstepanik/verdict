//! Integration tests: Agent delegation, AgentRegistry, DelegationPolicy, multi-agent pipelines
//!
//! All tests use StepAction::Custom for child agents — no LLM required.
//! Tests verify delegation depth, allowlists, tool scope inheritance,
//! step_result namespacing, schema validation, and audit event ordering.

use std::sync::Arc;
use verdict::prelude::*;
use serde_json::json;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn custom_step_simple(name: &str, output: &'static str) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |_ctx| Ok(StepOutput::new(output.into())))),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
    }
}

fn leaf_agent(name: &str, step_name: &str, output: &'static str) -> Agent {
    let step = custom_step_simple(step_name, output);
    let pipeline = Pipeline {
        name: format!("{name}_pipeline"),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    Agent {
        name: name.into(),
        description: format!("leaf agent {name}"),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    }
}

fn delegate_step(step_name: &str, agent_name: &str, input: serde_json::Value, max_depth: u32, allowed: Vec<String>) -> AgentStep {
    AgentStep {
        name: step_name.into(),
        guard_in: Guard::None,
        action: StepAction::DelegateAgent {
            agent: agent_name.into(),
            input,
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth,
                allowed_agents: allowed,
                require_output_schema: false,
                inherit_tool_scope: true,
                inherit_budget: false,
                require_user_approval: false,
            },
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
    }
}

// ─── Test 1: Two-level delegation depth increments in audit log ───────────────

#[tokio::test]
async fn test_delegation_two_level_depth_in_audit_log() {
    // C: leaf
    let agent_c = leaf_agent("C", "do_work", "C output");

    // B: delegates to C
    let b_step = delegate_step("delegate_to_c", "C", json!({}), 5, vec![]);
    let b_pipeline = Pipeline {
        name: "B_pipeline".into(),
        steps: vec![b_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent_b = Agent {
        name: "B".into(), description: "middle agent".into(),
        pipeline: b_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    // A: delegates to B
    let a_step = delegate_step("delegate_to_b", "B", json!({}), 5, vec![]);
    let a_pipeline = Pipeline {
        name: "A_pipeline".into(),
        steps: vec![a_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent_a = Agent {
        name: "A".into(), description: "top agent".into(),
        pipeline: a_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut reg = AgentRegistry::new();
    reg.register(agent_c);
    reg.register(agent_b);

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let result = runner.run(&a_pipeline, &agent_a, json!({})).await.unwrap();

    assert!(result.success);

    // Check that audit log has two DelegationStarted events with correct depths
    let starts: Vec<(String, String, u32)> = result.audit_log.entries().iter()
        .filter_map(|e| match &e.event {
            AuditEvent::DelegationStarted { parent_agent, child_agent, depth } =>
                Some((parent_agent.clone(), child_agent.clone(), *depth)),
            _ => None,
        }).collect();

    assert_eq!(starts.len(), 2, "expected 2 DelegationStarted events, got {starts:?}");
    assert_eq!(starts[0], ("A".into(), "B".into(), 1));
    assert_eq!(starts[1], ("B".into(), "C".into(), 2));
}

// ─── Test 2: max_depth=1 inside B blocks B→C delegation ─────────────────────

#[tokio::test]
async fn test_delegation_max_depth_blocks_grandchild() {
    let agent_c = leaf_agent("C", "do_work", "C output");

    // B: delegates to C but with max_depth=1 in the delegation policy
    let b_step = AgentStep {
        name: "delegate_to_c".into(),
        guard_in: Guard::None,
        action: StepAction::DelegateAgent {
            agent: "C".into(),
            input: json!({}),
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth: 1, // B is already at depth 1, so this blocks C (depth 2)
                allowed_agents: vec![],
                require_output_schema: false,
                inherit_tool_scope: true,
                inherit_budget: false,
                require_user_approval: false,
            },
        },
        guard_out: Guard::None, verdict: Verdict::None, tools: ToolSet::None,
        injection_protection: InjectionProtection::None, output_schema: None,
        dependencies: vec![], parallel: false,
    };
    let b_pipeline = Pipeline {
        name: "B_pipeline".into(), steps: vec![b_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent_b = Agent {
        name: "B".into(), description: "middle".into(),
        pipeline: b_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    // A delegates to B with Skip so we can inspect both outcomes
    let a_step = delegate_step("delegate_to_b", "B", json!({}), 5, vec![]);
    let a_pipeline = Pipeline {
        name: "A_pipeline".into(), steps: vec![a_step],
        on_failure: FailureMode::Skip, max_retries: 0,
    };
    let agent_a = Agent {
        name: "A".into(), description: "top".into(),
        pipeline: a_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut reg = AgentRegistry::new();
    reg.register(agent_c);
    reg.register(agent_b);

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let result = runner.run(&a_pipeline, &agent_a, json!({})).await.unwrap();

    // The B→C delegation fails, which cascades up, so delegate_to_b is failed
    assert!(!result.success);
    assert!(result.steps_failed.contains(&"delegate_to_b".to_string()),
        "delegation step should fail: {:?}", result.steps_failed);
}

// ─── Test 3: allowed_agents restricts which agents can be delegated to ────────

#[tokio::test]
async fn test_delegation_allowed_agents_blocks_disallowed_target() {
    let planner = leaf_agent("planner", "plan", "plan-out");
    let coder = leaf_agent("coder", "code", "code-out");

    let mut reg = AgentRegistry::new();
    reg.register(planner);
    reg.register(coder);

    // Step 1: try to delegate to coder (NOT in allowed list) → fails
    let blocked_step = AgentStep {
        name: "delegate_to_coder".into(),
        guard_in: Guard::None,
        action: StepAction::DelegateAgent {
            agent: "coder".into(),
            input: json!({}),
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth: 3,
                allowed_agents: vec!["planner".into()], // coder NOT allowed
                require_output_schema: false,
                inherit_tool_scope: true,
                inherit_budget: false,
                require_user_approval: false,
            },
        },
        guard_out: Guard::None, verdict: Verdict::None, tools: ToolSet::None,
        injection_protection: InjectionProtection::None, output_schema: None,
        dependencies: vec![], parallel: false,
    };
    // Step 2: delegate to planner (IS in allowed list) → succeeds
    let allowed_step = delegate_step("delegate_to_planner", "planner", json!({}), 3,
        vec!["planner".into()]);

    let pipeline = Pipeline {
        name: "p".into(),
        steps: vec![blocked_step, allowed_step],
        on_failure: FailureMode::Skip, // continue past failure so we see both outcomes
        max_retries: 0,
    };
    let agent = Agent {
        name: "root".into(), description: "root".into(),
        pipeline: pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(!result.success, "overall should fail because coder delegation failed");
    assert!(result.steps_failed.contains(&"delegate_to_coder".to_string()),
        "coder delegation must fail");
    assert!(result.steps_passed.contains(&"delegate_to_planner".to_string()),
        "planner delegation must succeed: {:?}", result.steps_passed);

    // planner's step result must exist; coder's must not
    assert!(result.step_results.contains_key("planner.plan"),
        "planner step result must be present: {:?}", result.step_results.keys().collect::<Vec<_>>());
    assert!(!result.step_results.contains_key("coder.code"),
        "coder step result must NOT be present");
}

// ─── Test 4: inherit_tool_scope=true lets child use parent's tool ─────────────

#[tokio::test]
async fn test_delegation_inherit_tool_scope_true_child_calls_parent_tool() {
    let parent_tool = FunctionTool::new(
        "local.parent_ping",
        "responds pong",
        json!({ "type": "object", "properties": {} }),
        |_args, _ctx| Box::pin(async move { Ok(ToolOutput::text("pong".to_string())) }),
    );
    let mut tool_reg = ToolRegistry::new();
    tool_reg.register(parent_tool);

    // Child agent has a ToolCall step targeting local.parent_ping
    let child_step = AgentStep {
        name: "use_tool".into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall { tool: "local.parent_ping".into(), args: json!({}) },
        guard_out: Guard::None, verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None, dependencies: vec![], parallel: false,
    };
    let child_pipeline = Pipeline {
        name: "child_pipeline".into(), steps: vec![child_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let child_agent = Agent {
        name: "child".into(), description: "child".into(),
        pipeline: child_pipeline.clone(),
        tools: ToolSet::Full, skills: SkillSet::default(),
        policy: AgentPolicy { allowed_tools: ToolSet::Full, ..Default::default() },
    };

    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(child_agent);

    // Parent delegates to child with inherit_tool_scope=true
    let parent_step = AgentStep {
        name: "delegate".into(),
        guard_in: Guard::None,
        action: StepAction::DelegateAgent {
            agent: "child".into(),
            input: json!({}),
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth: 3, allowed_agents: vec![],
                require_output_schema: false,
                inherit_tool_scope: true, // key: child gets parent's ToolRegistry
                inherit_budget: false, require_user_approval: false,
            },
        },
        guard_out: Guard::None, verdict: Verdict::None, tools: ToolSet::Full,
        injection_protection: InjectionProtection::None, output_schema: None,
        dependencies: vec![], parallel: false,
    };
    let parent_pipeline = Pipeline {
        name: "parent_pipeline".into(), steps: vec![parent_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let parent_agent = Agent {
        name: "parent".into(), description: "parent".into(),
        pipeline: parent_pipeline.clone(),
        tools: ToolSet::Full, skills: SkillSet::default(),
        policy: AgentPolicy { allowed_tools: ToolSet::Full, ..Default::default() },
    };

    let mut runner = PipelineRunner::with_registries(
        Arc::new(tool_reg),
        Arc::new(agent_reg),
    );
    let result = runner.run(&parent_pipeline, &parent_agent, json!({})).await.unwrap();

    assert!(result.success);
    // Child's tool step output should be "pong"
    assert!(result.step_results.contains_key("child.use_tool"),
        "child step result must be present: {:?}", result.step_results.keys().collect::<Vec<_>>());
    assert_eq!(result.step_results["child.use_tool"].output.raw, "pong");
}

// ─── Test 5: Child step results namespaced as "{agent}.{step}" ───────────────

#[tokio::test]
async fn test_delegation_step_results_namespaced_correctly() {
    // Worker agent has two steps
    let step_a = custom_step_simple("phase_a", "alpha");
    let step_b = custom_step_simple("phase_b", "beta");
    let worker_pipeline = Pipeline {
        name: "worker_pipeline".into(),
        steps: vec![step_a, step_b],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let worker = Agent {
        name: "worker".into(), description: "worker".into(),
        pipeline: worker_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut reg = AgentRegistry::new();
    reg.register(worker);

    let parent_step = delegate_step("delegate", "worker", json!({}), 3, vec![]);
    let parent_pipeline = Pipeline {
        name: "p".into(), steps: vec![parent_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let parent_agent = Agent {
        name: "parent".into(), description: "parent".into(),
        pipeline: parent_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let result = runner.run(&parent_pipeline, &parent_agent, json!({})).await.unwrap();

    assert!(result.success);

    // Namespaced results
    assert!(result.step_results.contains_key("worker.phase_a"),
        "worker.phase_a must be present: {:?}", result.step_results.keys().collect::<Vec<_>>());
    assert!(result.step_results.contains_key("worker.phase_b"),
        "worker.phase_b must be present");
    assert_eq!(result.step_results["worker.phase_a"].output.raw, "alpha");
    assert_eq!(result.step_results["worker.phase_b"].output.raw, "beta");

    // Parent's own delegation step present without namespace
    assert!(result.step_results.contains_key("delegate"),
        "parent delegation step must be present");

    // Unprefixed child step names must NOT appear
    assert!(!result.step_results.contains_key("phase_a"), "phase_a must not appear without namespace");
    assert!(!result.step_results.contains_key("phase_b"), "phase_b must not appear without namespace");
}

// ─── Test 6: Child pipeline failure causes parent step to fail ────────────────

#[tokio::test]
async fn test_delegation_child_failure_aborts_parent() {
    let failing_step = AgentStep {
        name: "fail".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| {
            Err(StepError::ActionFailed { reason: "child error".into() })
        })),
        guard_out: Guard::None, verdict: Verdict::None, tools: ToolSet::None,
        injection_protection: InjectionProtection::None, output_schema: None,
        dependencies: vec![], parallel: false,
    };
    let child_pipeline = Pipeline {
        name: "child_pipeline".into(), steps: vec![failing_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let child = Agent {
        name: "child".into(), description: "child".into(),
        pipeline: child_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut reg = AgentRegistry::new();
    reg.register(child);

    let parent_step = delegate_step("delegate", "child", json!({}), 3, vec![]);
    let parent_pipeline = Pipeline {
        name: "p".into(), steps: vec![parent_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let parent_agent = Agent {
        name: "parent".into(), description: "parent".into(),
        pipeline: parent_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let err = runner.run(&parent_pipeline, &parent_agent, json!({})).await.unwrap_err();

    match err {
        PipelineError::StepFailed { step, error } => {
            assert_eq!(step, "delegate");
            let msg = error.to_string();
            assert!(msg.contains("Delegation") || msg.contains("child") || msg.contains("failed"),
                "error should mention delegation/child failure: {msg}");
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
}

// ─── Test 7: Delegation to unknown agent returns DelegationFailed ─────────────

#[tokio::test]
async fn test_delegation_unknown_agent_fails_gracefully() {
    let reg = AgentRegistry::new(); // empty registry

    let parent_step = delegate_step("try_unknown", "nonexistent_agent", json!({}), 3, vec![]);
    let parent_pipeline = Pipeline {
        name: "p".into(), steps: vec![parent_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let parent_agent = Agent {
        name: "parent".into(), description: "parent".into(),
        pipeline: parent_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let err = runner.run(&parent_pipeline, &parent_agent, json!({})).await.unwrap_err();

    // Should get either DelegationFailed or StepFailed with delegation reason
    let msg = format!("{err:?}");
    assert!(
        matches!(err, PipelineError::DelegationFailed { .. } | PipelineError::StepFailed { .. }),
        "expected delegation-related error, got: {msg}"
    );
    assert!(
        msg.contains("nonexistent_agent") || msg.contains("not found") || msg.contains("failed"),
        "error should reference unknown agent: {msg}"
    );
}

// ─── Test 8: Sequential plan→code→review delegation, all results in step_results ─

#[tokio::test]
async fn test_delegation_sequential_plan_code_review() {
    let plan_agent = leaf_agent("plan_agent", "do_work", "plan-out");
    let code_agent = leaf_agent("code_agent", "do_work", "code-out");
    let review_agent = leaf_agent("review_agent", "do_work", "review-out");

    let mut reg = AgentRegistry::new();
    reg.register(plan_agent);
    reg.register(code_agent);
    reg.register(review_agent);

    let step1 = delegate_step("plan", "plan_agent", json!({}), 3, vec![]);
    let step2 = delegate_step("code", "code_agent", json!({}), 3, vec![]);
    let step3 = delegate_step("review", "review_agent", json!({}), 3, vec![]);

    let pipeline = Pipeline {
        name: "orchestrate".into(),
        steps: vec![step1, step2, step3],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = Agent {
        name: "orchestrator".into(), description: "orchestrator".into(),
        pipeline: pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert_eq!(result.step_results["plan_agent.do_work"].output.raw, "plan-out");
    assert_eq!(result.step_results["code_agent.do_work"].output.raw, "code-out");
    assert_eq!(result.step_results["review_agent.do_work"].output.raw, "review-out");

    // Audit log: 3 Started + 3 Completed pairs in order
    let delegation_events: Vec<(&str, String)> = result.audit_log.entries().iter()
        .filter_map(|e| match &e.event {
            AuditEvent::DelegationStarted { child_agent, .. } => Some(("start", child_agent.clone())),
            AuditEvent::DelegationCompleted { child_agent, .. } => Some(("complete", child_agent.clone())),
            _ => None,
        }).collect();

    assert_eq!(delegation_events, vec![
        ("start", "plan_agent".into()),
        ("complete", "plan_agent".into()),
        ("start", "code_agent".into()),
        ("complete", "code_agent".into()),
        ("start", "review_agent".into()),
        ("complete", "review_agent".into()),
    ], "delegation events must be in correct order: {delegation_events:?}");
}

// ─── Test 9: Downstream step reads child output from step_results ─────────────

#[tokio::test]
async fn test_delegation_downstream_step_reads_child_output() {
    let worker = leaf_agent("worker", "compute", "42");
    let mut reg = AgentRegistry::new();
    reg.register(worker);

    // Step 1: delegate to worker
    let delegate = delegate_step("delegate", "worker", json!({}), 3, vec![]);

    // Step 2: Custom step reads worker.compute from step_results
    let consume = AgentStep {
        name: "consume".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|ctx| {
            let v = ctx.step_results.get("worker.compute")
                .ok_or_else(|| StepError::ActionFailed { reason: "missing worker.compute".into() })?
                .output.raw.clone();
            Ok(StepOutput::new(format!("{v}{v}")))
        })),
        guard_out: Guard::None, verdict: Verdict::None, tools: ToolSet::None,
        injection_protection: InjectionProtection::None, output_schema: None,
        dependencies: vec![], parallel: false,
    };

    let pipeline = Pipeline {
        name: "p".into(),
        steps: vec![delegate, consume],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = Agent {
        name: "root".into(), description: "root".into(),
        pipeline: pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert_eq!(result.step_results["consume"].output.raw, "4242");
}

// ─── Test 10: Audit DelegationStarted always precedes DelegationCompleted ─────

#[tokio::test]
async fn test_delegation_audit_started_precedes_completed_invariant() {
    let agent_c = leaf_agent("C", "do_work", "C");

    let b_step = delegate_step("delegate_to_c", "C", json!({}), 5, vec![]);
    let b_pipeline = Pipeline {
        name: "B_pipeline".into(), steps: vec![b_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent_b = Agent {
        name: "B".into(), description: "B".into(),
        pipeline: b_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let a_step = delegate_step("delegate_to_b", "B", json!({}), 5, vec![]);
    let a_pipeline = Pipeline {
        name: "A_pipeline".into(), steps: vec![a_step],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent_a = Agent {
        name: "A".into(), description: "A".into(),
        pipeline: a_pipeline.clone(), tools: ToolSet::None,
        skills: SkillSet::default(), policy: AgentPolicy::default(),
    };

    let mut reg = AgentRegistry::new();
    reg.register(agent_c);
    reg.register(agent_b);

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(reg));
    let result = runner.run(&a_pipeline, &agent_a, json!({})).await.unwrap();

    // Verify pairing invariant: every Completed has a preceding Started
    use std::collections::HashMap;
    let mut open: HashMap<(String, String, u32), u32> = HashMap::new();
    for entry in result.audit_log.entries() {
        match &entry.event {
            AuditEvent::DelegationStarted { parent_agent, child_agent, depth } => {
                *open.entry((parent_agent.clone(), child_agent.clone(), *depth)).or_insert(0) += 1;
            }
            AuditEvent::DelegationCompleted { parent_agent, child_agent, depth } => {
                let key = (parent_agent.clone(), child_agent.clone(), *depth);
                let cnt = open.get_mut(&key)
                    .expect("DelegationCompleted without matching DelegationStarted");
                *cnt -= 1;
                if *cnt == 0 { open.remove(&key); }
            }
            AuditEvent::DelegationFailed { parent_agent, child_agent, depth, .. } => {
                let key = (parent_agent.clone(), child_agent.clone(), *depth);
                if let Some(cnt) = open.get_mut(&key) {
                    *cnt -= 1;
                    if *cnt == 0 { open.remove(&key); }
                }
            }
            _ => {}
        }
    }
    assert!(open.is_empty(), "unclosed delegations (Started without Completed): {open:?}");
}
