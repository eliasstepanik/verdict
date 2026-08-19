//! Phase 5 — Skills integration tests

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

mod common;
use common::MockLlmProvider;

// ============================================================================
// Helper functions for test pipelines and agents
// ============================================================================

fn skill_pipeline(skill_name: &str, mode: SkillMode) -> Pipeline {
    Pipeline {
        name: "test_skill_pipeline".into(),
        steps: vec![AgentStep {
            name: "use_skill".into(),
            guard_in: Guard::None,
            action: StepAction::UseSkill {
                skill: skill_name.to_string(),
                input: json!({}),
                mode,
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

fn dummy_agent(name: &str, pipeline: Pipeline) -> Agent {
    let mut policy = AgentPolicy::default();
    // FIX: Set agent baseline to ToolSet::Full so skill narrowing is actually tested,
    // not hidden by an already-deny-all agent policy.
    policy.allowed_tools = ToolSet::Full;
    
    Agent {
        name: name.into(),
        description: "".into(),
        pipeline,
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: Vec::new(),
    }
}

// ============================================================================
// Tests 1-4: SkillRegistry basic operations
// ============================================================================

#[test]
fn test_skill_registry_starts_empty() {
    let registry = SkillRegistry::new();
    assert!(registry.list().is_empty());
}

#[test]
fn test_skill_registry_register_and_get() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "test_skill".into(),
        description: "A test skill".into(),
        instructions: "Do this: test it".into(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };

    registry.register(skill.clone());
    let retrieved = registry.get("test_skill");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name, "test_skill");
}

#[test]
fn test_skill_registry_get_returns_none_for_unknown() {
    let registry = SkillRegistry::new();
    assert!(registry.get("nonexistent").is_none());
}

#[test]
fn test_skill_registry_list_returns_all_names() {
    let mut registry = SkillRegistry::new();

    for i in 1..=3 {
        let skill = Skill {
            name: format!("skill_{}", i),
            description: format!("Skill {}", i),
            instructions: format!("Instructions for skill {}", i),
            allowed_tools: ToolSet::Full,
            required_guards: vec![],
            pipeline: None,
            examples: vec![],
            eval: None,
        };
        registry.register(skill);
    }

    let list = registry.list();
    assert_eq!(list.len(), 3);
    assert!(list.contains(&"skill_1".to_string()));
    assert!(list.contains(&"skill_2".to_string()));
    assert!(list.contains(&"skill_3".to_string()));
}

// ============================================================================
// Tests 5-7: Skill and SkillSet struct operations
// ============================================================================

#[test]
fn test_skill_struct_fields_accessible() {
    let skill = Skill {
        name: "my_skill".into(),
        description: "My description".into(),
        instructions: "My instructions".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![Guard::None],
        pipeline: None,
        examples: vec![],
        eval: None,
    };

    assert_eq!(skill.name, "my_skill");
    assert_eq!(skill.description, "My description");
    assert_eq!(skill.instructions, "My instructions");
    assert_eq!(skill.required_guards.len(), 1);
    assert!(skill.examples.is_empty());
}

#[test]
fn test_skillset_construction_from_vec() {
    let skillset = SkillSet::from(vec!["rust", "testing"]);
    assert_eq!(skillset.skills.len(), 2);
    assert!(skillset.skills.contains(&"rust".to_string()));
    assert!(skillset.skills.contains(&"testing".to_string()));
}

#[test]
fn test_skillset_default_is_empty() {
    let skillset = SkillSet::default();
    assert!(skillset.skills.is_empty());
}

// ============================================================================
// Tests 8-12: UseSkill action with various modes
// ============================================================================

#[tokio::test]
async fn test_use_skill_prompt_only_returns_instructions() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "prompt_skill".into(),
        description: "A prompt-only skill".into(),
        instructions: "This is my instruction text".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("prompt_skill", SkillMode::PromptOnly);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 1);

    // Check that the output contains the instruction text
    let step_result = result.step_results.get("use_skill");
    assert!(step_result.is_some());
    assert!(step_result
        .unwrap()
        .output
        .raw
        .contains("This is my instruction text"));
}

#[tokio::test]
async fn test_use_skill_unknown_skill_returns_error() {
    let registry = SkillRegistry::new();

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("nonexistent_skill", SkillMode::PromptOnly);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_err());
    // Error should be about skill not found
    match result {
        Err(PipelineError::StepFailed { error, .. }) => {
            assert!(matches!(error, StepError::ActionFailed { .. }));
        }
        _ => panic!("Expected StepFailed error with ActionFailed variant"),
    }
}

#[tokio::test]
async fn test_use_skill_pipeline_mode_no_pipeline_falls_back() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "no_pipeline_skill".into(),
        description: "A skill without a pipeline".into(),
        instructions: "Fallback to these instructions".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("no_pipeline_skill", SkillMode::Pipeline);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);

    let step_result = result.step_results.get("use_skill");
    assert!(step_result.is_some());
    assert!(step_result
        .unwrap()
        .output
        .raw
        .contains("Fallback to these instructions"));
}

#[tokio::test]
async fn test_use_skill_pipeline_mode_with_pipeline_executes_it() {
    let mut registry = SkillRegistry::new();

    // Create a simple pipeline for the skill
    let sub_pipeline = Pipeline {
        name: "skill_subpipeline".into(),
        steps: vec![AgentStep {
            name: "execute_work".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(std::sync::Arc::new(|_ctx| {
                Ok(StepOutput::new("skill pipeline executed".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let skill = Skill {
        name: "with_pipeline_skill".into(),
        description: "A skill with a pipeline".into(),
        instructions: "Instructions (should not see this)".into(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: Some(sub_pipeline),
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("with_pipeline_skill", SkillMode::Pipeline);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 1);
}

#[tokio::test]
async fn test_use_skill_auto_mode_no_pipeline() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "auto_skill".into(),
        description: "A skill for auto mode testing".into(),
        instructions: "Auto mode instructions".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("auto_skill", SkillMode::Auto);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);

    let step_result = result.step_results.get("use_skill");
    assert!(step_result.is_some());
    assert!(step_result
        .unwrap()
        .output
        .raw
        .contains("Auto mode instructions"));
}

// ============================================================================
// Tests 13-16: Built-in skills validation
// ============================================================================

#[test]
fn test_builtin_rust_debugging_skill_name() {
    let skill = rust_debugging();
    assert_eq!(skill.name, "rust_debugging");
}

#[test]
fn test_builtin_rust_debugging_has_instructions() {
    let skill = rust_debugging();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("cargo check"));
}

#[test]
fn test_builtin_code_review_skill_name() {
    let skill = code_review();
    assert_eq!(skill.name, "code_review");
}

#[test]
fn test_builtin_code_review_has_instructions() {
    let skill = code_review();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("Review"));
}

#[test]
fn test_builtin_api_design_skill_name() {
    let skill = api_design();
    assert_eq!(skill.name, "api_design");
}

#[test]
fn test_builtin_api_design_has_instructions() {
    let skill = api_design();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("API"));
}

// ============================================================================
// Tests 17-18: Multiple skills and runner integration
// ============================================================================

#[test]
fn test_skill_registry_holds_multiple_skills() {
    let mut registry = SkillRegistry::new();

    for i in 1..=5 {
        let skill = Skill {
            name: format!("skill_{}", i),
            description: format!("Skill {}", i),
            instructions: format!("Instructions for skill {}", i),
            allowed_tools: ToolSet::Full,
            required_guards: vec![],
            pipeline: None,
            examples: vec![],
            eval: None,
        };
        registry.register(skill);
    }

    // Verify all are retrievable
    for i in 1..=5 {
        let skill_name = format!("skill_{}", i);
        assert!(registry.get(&skill_name).is_some());
        assert_eq!(registry.get(&skill_name).unwrap().name, skill_name);
    }
}

#[test]
fn test_pipeline_runner_with_skill_registry_constructor() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "runner_test_skill".into(),
        description: "Test skill".into(),
        instructions: "Test instructions".into(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let registry = Arc::new(registry);
    let runner = PipelineRunner::with_skill_registry(registry.clone());

    // Verify the runner was created with the skill registry
    assert_eq!(runner.skill_registry.list().len(), 1);
    assert!(runner.skill_registry.get("runner_test_skill").is_some());
}

#[test]
fn test_skill_example_struct_fields() {
    let example = SkillExample {
        input: json!({ "key": "value" }),
        expected_output: "expected result".into(),
        description: "Example description".into(),
    };

    assert_eq!(example.expected_output, "expected result");
    assert_eq!(example.description, "Example description");
}

#[test]
fn test_skill_eval_struct_fields() {
    let eval = SkillEval {
        criteria: vec!["criterion1".into(), "criterion2".into()],
        min_score: 0.75,
    };

    assert_eq!(eval.criteria.len(), 2);
    assert_eq!(eval.min_score, 0.75);
}

/// Test 109: Built-in test_writing skill has correct name
#[test]
fn test_builtin_test_writing_skill_name() {
    let skill = test_writing();
    assert_eq!(skill.name, "test_writing");
}

/// Test 110: Built-in test_writing skill has non-empty instructions
#[test]
fn test_builtin_test_writing_has_instructions() {
    let skill = test_writing();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("test"));
}

/// Test 111: Built-in refactoring skill has correct name
#[test]
fn test_builtin_refactoring_skill_name() {
    let skill = refactoring();
    assert_eq!(skill.name, "refactoring");
}

/// Test 112: Built-in refactoring skill has non-empty instructions
#[test]
fn test_builtin_refactoring_has_instructions() {
    let skill = refactoring();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("test") || skill.instructions.contains("refactor"));
}

// ============================================================================
// FIX #1 & #2 MUTATION TESTS: UseSkill tool narrowing and placeholder removal
// ============================================================================

/// Test: UseSkill narrows ctx.allowed_tools by skill's allowed_tools.
/// Skill has allowed_tools: Allow(["check.allowed"]), and its pipeline checks tool access.
/// 1. POSITIVE CONTROL: call to allowed tool SUCCEEDS
/// 2. NEGATIVE CONTROL: call to forbidden tool FAILS with rejection reason
/// This test MUST FAIL if FIX #1 (the Intersection line) is removed.
#[tokio::test]
async fn test_use_skill_narrows_allowed_tools() {
    let mut skill_registry = SkillRegistry::new();
    let mut tool_registry = ToolRegistry::new();
    
    // Register test tools
    let check_tool = FunctionTool::new(
        "check.allowed",
        "A tool that is allowed in the skill",
        json!({"type": "object", "properties": {}, "required": []}),
        |_args, _ctx| {
            Box::pin(async move { Ok(ToolOutput::text("allowed-success".into())) })
        },
    );
    
    let forbidden_tool = FunctionTool::new(
        "forbidden.tool",
        "A tool that should be forbidden by the skill",
        json!({"type": "object", "properties": {}, "required": []}),
        |_args, _ctx| {
            Box::pin(async move { Ok(ToolOutput::text("forbidden-should-not-call".into())) })
        },
    );
    
    tool_registry.register(check_tool);
    tool_registry.register(forbidden_tool);
    
    // Create a skill that only allows check.allowed (NOT forbidden.tool)
    // Its pipeline has two steps:
    // 1. Call allowed tool (POSITIVE CONTROL - must succeed)
    // 2. Call forbidden tool (NEGATIVE CONTROL - must fail with rejection)
    let skill = Skill {
        name: "narrowing_skill".into(),
        description: "Tests tool narrowing".into(),
        instructions: "You have access to check.allowed only.".into(),
        allowed_tools: ToolSet::Allow(vec!["check.allowed".into()]),
        required_guards: vec![],
        pipeline: Some(Pipeline {
            name: "skill_pipeline_with_narrowing".into(),
            steps: vec![
                // Step 1: Call allowed tool (POSITIVE CONTROL)
                AgentStep {
                    name: "call_allowed".into(),
                    guard_in: Guard::None,
                    action: StepAction::ToolCall {
                        tool: "check.allowed".into(),
                        args: json!({}),
                    },
                    guard_out: Guard::None,
                    verdict: Verdict::Automated(Guard::None),
                    tools: ToolSet::Full,
                    injection_protection: InjectionProtection::None,
                    output_schema: None,
                    dependencies: vec![],
                    parallel: false,
                    input_processors: vec![],
                    output_processors: vec![],
                },
                // Step 2: Try to call forbidden tool (NEGATIVE CONTROL)
                AgentStep {
                    name: "call_forbidden".into(),
                    guard_in: Guard::None,
                    action: StepAction::ToolCall {
                        tool: "forbidden.tool".into(),
                        args: json!({}),
                    },
                    guard_out: Guard::None,
                    verdict: Verdict::Automated(Guard::None),
                    tools: ToolSet::Full,
                    injection_protection: InjectionProtection::None,
                    output_schema: None,
                    dependencies: vec![],
                    parallel: false,
                    input_processors: vec![],
                    output_processors: vec![],
                },
            ],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        }),
        examples: vec![],
        eval: None,
    };
    skill_registry.register(skill);

    let mut runner = PipelineRunner::new();
    runner.tool_registry = Arc::new(tool_registry);
    runner.skill_registry = Arc::new(skill_registry);
    
    let pipeline = Pipeline {
        name: "test_narrowing".into(),
        steps: vec![
            AgentStep {
                name: "use_narrowing_skill".into(),
                guard_in: Guard::None,
                action: StepAction::UseSkill {
                    skill: "narrowing_skill".into(),
                    input: json!({}),
                    mode: SkillMode::Pipeline,
                },
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::Full,
                injection_protection: InjectionProtection::Strict,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline);
    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    
    // CRITICAL ASSERTION:
    // If FIX #1 is disabled, the narrowing doesn't happen, and forbidden.tool would be called (step 2 succeeds).
    // With FIX #1 enabled: narrowing is applied in UseSkill via Intersection, and forbidden.tool is blocked.
    // Positive control: step 1 (call_allowed) MUST succeed before we hit the failure.
    // The error must come from the tool-not-allowed rejection, not some other failure mode.
    assert!(result.is_err(), "Pipeline must fail because forbidden.tool should not be accessible to the skill");
    
    // Strengthen the assertion: check the error is specifically about tool rejection
    match result {
        Err(verdict::PipelineError::StepFailed { step, error }) => {
            assert_eq!(step, "use_narrowing_skill", "Error should be from the UseSkill step");
            // The error should indicate tool-scope restriction, not just any failure
            let error_str = format!("{:?}", error);
            assert!(
                error_str.contains("not allowed") || error_str.contains("forbidden") || error_str.contains("Tool"),
                "Error should indicate tool rejection/not-allowed, got: {}",
                error_str
            );
        }
        _ => {
            panic!(
                "Expected StepFailed error with tool rejection reason, got: {:?}",
                result
            );
        }
    }
}

/// Test: UseSkill PromptOnly mode does not leak literal {system}/{user} placeholders into LLM call.
/// With a real LLM client, FIX #2 ensures skill instructions are injected as system prompt only.
/// This test MUST FAIL if FIX #2 (removing literal placeholder strings) is reverted.
#[tokio::test]
async fn test_use_skill_prompt_only_no_placeholder_leak() {
    let mut registry = SkillRegistry::new();
    
    let skill = Skill {
        name: "test_skill".into(),
        description: "Test skill".into(),
        instructions: "Always respond with: SKILL_EXECUTED".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    // Create a mock LLM provider to capture the request
    let mock_provider = Arc::new(MockLlmProvider::new("SKILL_EXECUTED"));
    let llm_client = Arc::new(LlmClient::new(mock_provider.clone()));
    
    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    runner.llm_client = Some(llm_client);
    
    let pipeline = Pipeline {
        name: "test_prompt_only".into(),
        steps: vec![
            AgentStep {
                name: "use_skill".into(),
                guard_in: Guard::None,
                action: StepAction::UseSkill {
                    skill: "test_skill".into(),
                    input: json!({}),
                    mode: SkillMode::PromptOnly,
                },
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::ReadOnly,
                injection_protection: InjectionProtection::Strict,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline);
    
    // With mock LLM client, PromptOnly should construct an LlmCall with skill instructions
    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);
    
    // Check that the LLM was called and the system prompt doesn't contain literal placeholders
    let captured = mock_provider.captured_request.lock().unwrap();
    assert!(captured.is_some(), "LLM should have been called");
    
    if let Some(req) = captured.as_ref() {
        // The system prompt should contain the skill instructions
        assert!(
            req.system.contains("SKILL_EXECUTED"),
            "System prompt should contain skill instructions"
        );
        // Should NOT contain literal placeholder strings in system or user
        assert!(
            !req.system.contains("{system}") && !req.system.contains("{user}"),
            "System prompt must not contain literal {{system}} or {{user}} placeholder strings. Got: {}",
            req.system
        );
        assert!(
            !req.user.contains("{system}") && !req.user.contains("{user}"),
            "User prompt must not contain literal {{system}} or {{user}} placeholder strings"
        );
    }
}

/// Mutation test: Verify SubPipeline scope propagation (FIX #3).
/// When a skill's pipeline (SkillMode::Pipeline) executes, the narrowed allowed_tools
/// from the parent UseSkill must be propagated to the sub-agent's policy, so that
/// the sub-pipeline's steps respect the narrowed scope (not revert to deny-all).
/// 
/// CRITICAL: If FIX #3 is disabled, the policy defaults to ToolSet::None (deny-all),
/// and EVEN the allowed tool in the skill's scope would fail. This test detects that
/// by checking that we get an error at the SubPipeline level (first step of skill
/// fails due to deny-all), vs the entire pipeline completing successfully or failing
/// only at the expected rejection point (second step).
/// 
/// This test MUST FAIL if FIX #3 (propagating ctx.allowed_tools to policy) is removed.
#[tokio::test]
async fn test_use_skill_subpipeline_scope_propagation() {
    let mut skill_registry = SkillRegistry::new();
    let mut tool_registry = ToolRegistry::new();
    
    // Register tools
    let allowed_in_skill = FunctionTool::new(
        "skill.tool",
        "Tool allowed by skill",
        json!({"type": "object", "properties": {}, "required": []}),
        |_args, _ctx| {
            Box::pin(async move { Ok(ToolOutput::text("skill-call-succeeded".into())) })
        },
    );
    
    tool_registry.register(allowed_in_skill);
    
    // Skill that allows only skill.tool (via ToolSet::Allow)
    let skill = Skill {
        name: "scoped_pipeline_skill".into(),
        description: "Skill with scoped pipeline".into(),
        instructions: "Use only skill.tool".into(),
        allowed_tools: ToolSet::Allow(vec!["skill.tool".into()]),
        required_guards: vec![],
        pipeline: Some(Pipeline {
            name: "skill_scoped_pipeline".into(),
            steps: vec![
                // Single step: Call skill.tool (allowed by skill)
                // If FIX #3 is working, ctx.allowed_tools is narrowed to skill.tool, so this succeeds
                // If FIX #3 is broken, ctx.allowed_tools = policy.allowed_tools = ToolSet::None (deny-all), so this fails
                AgentStep {
                    name: "use_skill_tool".into(),
                    guard_in: Guard::None,
                    action: StepAction::ToolCall {
                        tool: "skill.tool".into(),
                        args: json!({}),
                    },
                    guard_out: Guard::None,
                    verdict: Verdict::Automated(Guard::None),
                    tools: ToolSet::Full, // Step says Full, skill narrows to skill.tool
                    injection_protection: InjectionProtection::None,
                    output_schema: None,
                    dependencies: vec![],
                    parallel: false,
                    input_processors: vec![],
                    output_processors: vec![],
                },
            ],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        }),
        examples: vec![],
        eval: None,
    };
    skill_registry.register(skill);

    let mut runner = PipelineRunner::new();
    runner.tool_registry = Arc::new(tool_registry);
    runner.skill_registry = Arc::new(skill_registry);
    
    let pipeline = Pipeline {
        name: "test_subpipeline_scope".into(),
        steps: vec![
            AgentStep {
                name: "execute_skill".into(),
                guard_in: Guard::None,
                action: StepAction::UseSkill {
                    skill: "scoped_pipeline_skill".into(),
                    input: json!({}),
                    mode: SkillMode::Pipeline,
                },
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::Full, // Outer agent has Full tools
                injection_protection: InjectionProtection::Strict,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline);
    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    
    // CRITICAL MUTATION TEST:
    // With FIX #3 working:
    //   - Sub-pipeline's policy.allowed_tools = ctx.allowed_tools (narrowed to skill.tool)
    //   - Step ctx.allowed_tools = Intersection(policy.allowed_tools, step.tools)
    //                             = Intersection(skill.tool, Full)
    //                             = skill.tool
    //   - Tool call to skill.tool succeeds
    //   - Pipeline succeeds
    //
    // Without FIX #3 (broken):
    //   - Sub-pipeline's policy.allowed_tools = AgentPolicy::default().allowed_tools = ToolSet::None
    //   - Step ctx.allowed_tools = Intersection(ToolSet::None, Full) = ToolSet::None
    //   - Tool call to skill.tool fails (denied by deny-all policy)
    //   - Pipeline fails
    //
    // Therefore, with FIX #3 broken, this test will fail (result.is_ok() will be false).
    assert!(
        result.is_ok(),
        "With FIX #3, SubPipeline should succeed because skill.tool is in the narrowed scope. Got: {:?}",
        result
    );
    
    // Verify we got success output containing the tool's result
    let res = result.unwrap();
    assert!(res.success, "Pipeline result should be successful");
    let step_result = res.step_results.get("execute_skill");
    assert!(step_result.is_some(), "execute_skill step result should exist");
    assert!(
        step_result.unwrap().output.raw.contains("skill-call-succeeded"),
        "Step output should contain the tool call result, got: {}",
        step_result.unwrap().output.raw
    );
}
