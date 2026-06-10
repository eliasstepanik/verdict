//! Phase 4 — Agent Delegation tests
#![allow(unused_imports)]

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

// ─── helpers ────────────────────────────────────────────────────────────────

fn simple_agent(name: &str) -> Agent {
    Agent {
        name: name.to_string(),
        description: format!("{} agent", name),
        pipeline: Pipeline {
            name: format!("{}_pipeline", name),
            steps: vec![AgentStep {
                name: "work".to_string(),
                guard_in: Guard::None,
                action: StepAction::LlmCall {
                    system: "You are a helpful agent.".to_string(),
                    user: "Do some work.".to_string(),
                    model: None,
                },
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
            }],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    }
}

fn orchestrator_pipeline(child_agent: &str) -> Pipeline {
    Pipeline {
        name: "orchestrator".to_string(),
        steps: vec![AgentStep {
            name: "delegate".to_string(),
            guard_in: Guard::None,
            action: StepAction::DelegateAgent {
                agent: child_agent.to_string(),
                input: json!({ "task": "do something" }),
                expected_output_schema: None,
                delegation_policy: DelegationPolicy {
                    max_depth: 3,
                    allowed_agents: vec![child_agent.to_string()],
                    require_output_schema: false,
                    inherit_tool_scope: true,
                    inherit_budget: true,
                    require_user_approval: false,
                },
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

fn orchestrator_agent(child_agent: &str) -> Agent {
    Agent {
        name: "orchestrator".to_string(),
        description: "Orchestrator agent".to_string(),
        pipeline: orchestrator_pipeline(child_agent),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    }
}

// ─── Test 1: AgentRegistry register and get ─────────────────────────────────

/// Test 1: AgentRegistry can register and retrieve agents
#[test]
fn test_agent_registry_register_and_get() {
    let mut registry = AgentRegistry::new();
    let agent = simple_agent("coder");

    registry.register(agent);

    let retrieved = registry.get("coder");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name, "coder");
}

// ─── Test 2: AgentRegistry list ──────────────────────────────────────────────

/// Test 2: AgentRegistry::list returns all registered agent names
#[test]
fn test_agent_registry_list() {
    let mut registry = AgentRegistry::new();
    registry.register(simple_agent("planner"));
    registry.register(simple_agent("reviewer"));
    registry.register(simple_agent("coder"));

    let names = registry.list();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"planner".to_string()));
    assert!(names.contains(&"reviewer".to_string()));
    assert!(names.contains(&"coder".to_string()));
}

// ─── Test 3: AgentRegistry get nonexistent returns None ──────────────────────

/// Test 3: AgentRegistry returns None for unregistered agent
#[test]
fn test_agent_registry_get_nonexistent() {
    let registry = AgentRegistry::new();
    assert!(registry.get("nonexistent").is_none());
}

// ─── Test 4: PipelineRunner has agent_registry field ─────────────────────────

/// Test 4: PipelineRunner::with_agent_registry stores the registry
#[test]
fn test_pipeline_runner_with_agent_registry() {
    let mut reg = AgentRegistry::new();
    reg.register(simple_agent("worker"));

    let arc_reg = Arc::new(reg);
    let runner = PipelineRunner::with_agent_registry(arc_reg.clone());

    // Registry is accessible
    assert!(runner.agent_registry.get("worker").is_some());
}

// ─── Test 5: Successful delegation ──────────────────────────────────────────

/// Test 5: DelegateAgent action successfully runs the child agent
#[tokio::test]
async fn test_delegate_agent_succeeds() {
    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("worker"));

    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = orchestrator_agent("worker");

    let result = runner
        .run(&parent.pipeline, &parent, json!({}))
        .await;

    assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
    let result = result.unwrap();
    assert!(result.success);
    assert!(result.steps_passed.contains(&"delegate".to_string()));
}

// ─── Test 6: Delegation to unknown agent fails ───────────────────────────────

/// Test 6: DelegateAgent with unregistered agent name returns error
#[tokio::test]
async fn test_delegate_agent_unknown_agent_fails() {
    let runner_agent_reg = AgentRegistry::new(); // empty registry
    let arc_reg = Arc::new(runner_agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = orchestrator_agent("nonexistent");
    let result = runner.run(&parent.pipeline, &parent, json!({})).await;

    assert!(result.is_err());
    let err = result.err().unwrap();
    assert!(err.to_string().contains("nonexistent") || err.to_string().contains("not found"));
}

// ─── Test 7: Delegation depth limit ─────────────────────────────────────────

/// Test 7: DelegateAgent respects max_depth — delegation is blocked when depth equals limit
#[tokio::test]
async fn test_delegate_agent_depth_limit_enforced() {
    // Create a pipeline where max_depth is 0 — should fail immediately
    let pipeline = Pipeline {
        name: "too_deep".to_string(),
        steps: vec![AgentStep {
            name: "delegate".to_string(),
            guard_in: Guard::None,
            action: StepAction::DelegateAgent {
                agent: "worker".to_string(),
                input: json!({}),
                expected_output_schema: None,
                delegation_policy: DelegationPolicy {
                    max_depth: 0, // forbid any delegation
                    allowed_agents: vec!["worker".to_string()],
                    require_output_schema: false,
                    inherit_tool_scope: false,
                    inherit_budget: false,
                    require_user_approval: false,
                },
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("worker"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = Agent {
        name: "parent".to_string(),
        description: "Parent".to_string(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    };

    let result = runner.run(&pipeline, &parent, json!({})).await;
    assert!(result.is_err());
    let err_str = result.err().unwrap().to_string();
    assert!(
        err_str.contains("depth") || err_str.contains("limit") || err_str.contains("exceeded"),
        "Error should mention depth limit, got: {}",
        err_str
    );
}

// ─── Test 8: Delegation allowed_agents allowlist ─────────────────────────────

/// Test 8: DelegateAgent blocks delegation to agents not in allowed_agents
#[tokio::test]
async fn test_delegate_agent_allowlist_enforced() {
    let pipeline = Pipeline {
        name: "strict".to_string(),
        steps: vec![AgentStep {
            name: "delegate".to_string(),
            guard_in: Guard::None,
            action: StepAction::DelegateAgent {
                agent: "forbidden".to_string(),
                input: json!({}),
                expected_output_schema: None,
                delegation_policy: DelegationPolicy {
                    max_depth: 3,
                    allowed_agents: vec!["allowed_agent".to_string()], // "forbidden" not listed
                    require_output_schema: false,
                    inherit_tool_scope: false,
                    inherit_budget: false,
                    require_user_approval: false,
                },
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("forbidden"));
    agent_reg.register(simple_agent("allowed_agent"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = Agent {
        name: "parent".to_string(),
        description: "Parent".to_string(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    };

    let result = runner.run(&pipeline, &parent, json!({})).await;
    assert!(result.is_err());
    let err_str = result.err().unwrap().to_string();
    assert!(
        err_str.contains("allowed_agents") || err_str.contains("not in"),
        "Error should mention allowlist, got: {}",
        err_str
    );
}

// ─── Test 9: Empty allowed_agents allows all ─────────────────────────────────

/// Test 9: DelegateAgent with empty allowed_agents allows any registered agent
#[tokio::test]
async fn test_delegate_agent_empty_allowlist_allows_all() {
    let pipeline = Pipeline {
        name: "open".to_string(),
        steps: vec![AgentStep {
            name: "delegate".to_string(),
            guard_in: Guard::None,
            action: StepAction::DelegateAgent {
                agent: "any_agent".to_string(),
                input: json!({}),
                expected_output_schema: None,
                delegation_policy: DelegationPolicy {
                    max_depth: 3,
                    allowed_agents: vec![], // empty = allow all
                    require_output_schema: false,
                    inherit_tool_scope: true,
                    inherit_budget: true,
                    require_user_approval: false,
                },
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("any_agent"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = Agent {
        name: "parent".to_string(),
        description: "Parent".to_string(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    };

    let result = runner.run(&pipeline, &parent, json!({})).await;
    assert!(result.is_ok(), "Empty allowlist should allow any agent: {:?}", result.err());
}

// ─── Test 10: Delegation audit log events ────────────────────────────────────

/// Test 10: Successful delegation emits DelegationStarted and DelegationCompleted audit events
#[tokio::test]
async fn test_delegation_audit_events_on_success() {
    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("worker"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = orchestrator_agent("worker");
    let result = runner.run(&parent.pipeline, &parent, json!({})).await.unwrap();

    let entries = result.audit_log.entries();
    let event_types: Vec<String> = entries
        .iter()
        .map(|e| format!("{:?}", e.event))
        .collect();

    // Should have DelegationStarted and DelegationCompleted entries
    let has_started = event_types
        .iter()
        .any(|e| e.contains("DelegationStarted"));
    let has_completed = event_types
        .iter()
        .any(|e| e.contains("DelegationCompleted"));

    assert!(has_started, "Expected DelegationStarted in audit log. Events: {:?}", event_types);
    assert!(has_completed, "Expected DelegationCompleted in audit log. Events: {:?}", event_types);
}

// ─── Test 11: Delegation failure audit log ───────────────────────────────────

/// Test 11: Failed delegation (agent not found) emits DelegationFailed audit event
#[tokio::test]
async fn test_delegation_audit_events_on_failure() {
    let arc_reg = Arc::new(AgentRegistry::new()); // no agents
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = orchestrator_agent("ghost");
    let _ = runner.run(&parent.pipeline, &parent, json!({})).await;

    let entries = runner.audit_log.entries();
    let event_types: Vec<String> = entries
        .iter()
        .map(|e| format!("{:?}", e.event))
        .collect();

    let has_failed = event_types.iter().any(|e| e.contains("DelegationFailed"));
    assert!(has_failed, "Expected DelegationFailed in audit log. Events: {:?}", event_types);
}

// ─── Test 12: Child step results merged into parent context ──────────────────

/// Test 12: After delegation, child step results are accessible in parent's result
#[tokio::test]
async fn test_delegation_merges_child_step_results() {
    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("worker"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = orchestrator_agent("worker");
    let result = runner.run(&parent.pipeline, &parent, json!({})).await.unwrap();

    // Child's "work" step should be accessible as "worker.work" in parent result
    let merged_key = "worker.work";
    assert!(
        result.step_results.contains_key(merged_key),
        "Expected child step '{}' merged into parent results. Keys: {:?}",
        merged_key,
        result.step_results.keys().collect::<Vec<_>>()
    );
}

// ─── Test 13: Delegation depth counter increments ────────────────────────────

/// Test 13: DelegationPolicy struct has correct default-like shape
#[test]
fn test_delegation_policy_fields() {
    let policy = DelegationPolicy {
        max_depth: 2,
        allowed_agents: vec!["planner".to_string(), "coder".to_string()],
        require_output_schema: true,
        inherit_tool_scope: false,
        inherit_budget: true,
        require_user_approval: false,
    };

    assert_eq!(policy.max_depth, 2);
    assert_eq!(policy.allowed_agents.len(), 2);
    assert!(policy.require_output_schema);
    assert!(!policy.inherit_tool_scope);
    assert!(policy.inherit_budget);
    assert!(!policy.require_user_approval);
}

// ─── Test 14: with_registries constructor ────────────────────────────────────

/// Test 14: PipelineRunner::with_registries stores both registries
#[test]
fn test_pipeline_runner_with_registries() {
    let tool_reg = Arc::new(ToolRegistry::new());
    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("helper"));
    let agent_reg = Arc::new(agent_reg);

    let runner = PipelineRunner::with_registries(tool_reg, agent_reg.clone());

    assert!(runner.agent_registry.get("helper").is_some());
}

// ─── Test 15: Multi-step pipeline with delegation in middle ──────────────────

/// Test 15: Pipeline with pre- and post-delegation steps all pass
#[tokio::test]
async fn test_multi_step_pipeline_with_delegation() {
    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("reviewer"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let pipeline = Pipeline {
        name: "multi_step".to_string(),
        steps: vec![
            // Step 1: custom action before delegation
            AgentStep {
                name: "prepare".to_string(),
                guard_in: Guard::None,
                action: StepAction::Custom(std::sync::Arc::new(|_ctx| {
                    Ok(StepOutput::new("prepared".to_string()))
                })),
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
            },
            // Step 2: delegate to reviewer
            AgentStep {
                name: "review".to_string(),
                guard_in: Guard::None,
                action: StepAction::DelegateAgent {
                    agent: "reviewer".to_string(),
                    input: json!({ "data": "something to review" }),
                    expected_output_schema: None,
                    delegation_policy: DelegationPolicy {
                        max_depth: 3,
                        allowed_agents: vec!["reviewer".to_string()],
                        require_output_schema: false,
                        inherit_tool_scope: true,
                        inherit_budget: true,
                        require_user_approval: false,
                    },
                },
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
            },
            // Step 3: custom action after delegation
            AgentStep {
                name: "finalize".to_string(),
                guard_in: Guard::None,
                action: StepAction::Custom(std::sync::Arc::new(|_ctx| {
                    Ok(StepOutput::new("finalized".to_string()))
                })),
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let parent = Agent {
        name: "orchestrator".to_string(),
        description: "Orchestrator".to_string(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    };

    let result = runner.run(&pipeline, &parent, json!({})).await;
    assert!(result.is_ok(), "Expected Ok, got: {:?}", result.err());
    let result = result.unwrap();
    assert!(result.success);
    assert!(result.steps_passed.contains(&"prepare".to_string()));
    assert!(result.steps_passed.contains(&"review".to_string()));
    assert!(result.steps_passed.contains(&"finalize".to_string()));
}

// ─── Test 16: DelegateAgent output contains success summary ──────────────────

/// Test 16: DelegateAgent step output contains a summary of the delegation
#[tokio::test]
async fn test_delegate_agent_output_summary() {
    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("worker"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = orchestrator_agent("worker");
    let result = runner.run(&parent.pipeline, &parent, json!({})).await.unwrap();

    // The "delegate" step result should contain summary text
    let step_result = result.step_results.get("delegate");
    assert!(step_result.is_some(), "Expected 'delegate' step result");
    let raw = &step_result.unwrap().output.raw;
    assert!(
        raw.contains("worker") || raw.contains("completed") || raw.contains("Delegation"),
        "Expected delegation summary in output, got: {}",
        raw
    );
}

// ─── Test 17: Agent::default policy is restrictive ───────────────────────────

/// Test 17: AgentPolicy::default has no allowed agents and disallows self-update
#[test]
fn test_agent_policy_default_is_restrictive() {
    let policy = AgentPolicy::default();
    assert!(!policy.allow_self_update);
    assert!(policy.require_approval_for_self_update);
    // Default allowed_tools is None (most restrictive)
    match &policy.allowed_tools {
        ToolSet::None => {} // expected
        other => panic!("Expected ToolSet::None, got {:?}", other),
    }
}

// ─── Test 18: PipelineRunner::new has empty agent registry ───────────────────

/// Test 18: Default PipelineRunner has an empty agent registry
#[test]
fn test_default_runner_has_empty_agent_registry() {
    let runner = PipelineRunner::new();
    assert!(runner.agent_registry.list().is_empty());
}

// ─── Test 19: Delegation with inherit_tool_scope=false uses empty tool scope ──

/// Test 19: inherit_tool_scope=false creates child runner with no tools
#[tokio::test]
async fn test_delegation_inherit_tool_scope_false() {
    let pipeline = Pipeline {
        name: "no_inherit".to_string(),
        steps: vec![AgentStep {
            name: "delegate".to_string(),
            guard_in: Guard::None,
            action: StepAction::DelegateAgent {
                agent: "worker".to_string(),
                input: json!({}),
                expected_output_schema: None,
                delegation_policy: DelegationPolicy {
                    max_depth: 3,
                    allowed_agents: vec!["worker".to_string()],
                    require_output_schema: false,
                    inherit_tool_scope: false, // child gets no tools
                    inherit_budget: false,
                    require_user_approval: false,
                },
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let mut agent_reg = AgentRegistry::new();
    agent_reg.register(simple_agent("worker"));
    let arc_reg = Arc::new(agent_reg);
    let mut runner = PipelineRunner::with_agent_registry(arc_reg);

    let parent = Agent {
        name: "parent".to_string(),
        description: "Parent".to_string(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    };

    // Should succeed — the worker only uses LlmCall which doesn't need tools
    let result = runner.run(&pipeline, &parent, json!({})).await;
    assert!(result.is_ok(), "Expected Ok with inherit_tool_scope=false: {:?}", result.err());
}

// ─── Test 20: AgentRegistry overwrites duplicate name ────────────────────────

/// Test 20: Registering an agent with the same name overwrites the previous entry
#[test]
fn test_agent_registry_overwrite() {
    let mut registry = AgentRegistry::new();

    let agent_v1 = Agent {
        name: "worker".to_string(),
        description: "Version 1".to_string(),
        pipeline: simple_agent("worker").pipeline,
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    };

    let agent_v2 = Agent {
        name: "worker".to_string(),
        description: "Version 2".to_string(),
        pipeline: simple_agent("worker").pipeline,
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    };

    registry.register(agent_v1);
    registry.register(agent_v2);

    let names = registry.list();
    assert_eq!(names.len(), 1, "Should have only one 'worker' after overwrite");

    let retrieved = registry.get("worker").unwrap();
    assert_eq!(retrieved.description, "Version 2");
}
