//! Tests for Skill examples injection and evaluation features
//! 
//! Verifies that:
//! 1. Skill examples are injected into the system prompt as few-shot examples (uses real MockLlmProvider)
//! 2. Skill evaluation runs after step execution and attaches results
//! 3. min_score threshold comparison is genuine (not just formatted)
//! 4. Skills without examples or eval work as before (regression test)

use serde_json::json;
use verdict::prelude::*;
use std::sync::Arc;

mod common;
use common::MockLlmProvider;

fn dummy_agent(name: &str, pipeline: Pipeline) -> Agent {
    let mut policy = AgentPolicy::default();
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

/// Test that skill examples are actually injected into the system prompt sent to the LLM.
/// This uses MockLlmProvider to capture the real LlmRequest and verify the prompt contains
/// the example content.
#[tokio::test]
async fn test_skill_examples_injected_into_system_prompt() {
    let skill = Skill {
        name: "example_skill".to_string(),
        description: "A skill with examples".to_string(),
        instructions: "Complete the following tasks according to the examples.".to_string(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![
            SkillExample {
                input: json!({"task": "task1"}),
                expected_output: "Successfully completed task1".to_string(),
                description: "Simple example demonstrating task completion".to_string(),
            },
            SkillExample {
                input: json!({"task": "task2"}),
                expected_output: "Successfully completed task2".to_string(),
                description: "Complex example with edge case handling".to_string(),
            },
        ],
        eval: None,
    };

    // Create mock LLM provider that captures requests
    let mock_llm = MockLlmProvider::new("Step executed successfully");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm.clone())));
    
    let mut runner = PipelineRunner::new();
    runner.llm_client = Some(llm_client);

    // Register the skill
    let mut skill_registry = SkillRegistry::new();
    skill_registry.register(skill.clone());
    runner.skill_registry = Arc::new(skill_registry);

    // Create a pipeline with a UseSkill step
    let step = AgentStep {
        name: "skill_step".to_string(),
        guard_in: Guard::None,
        action: StepAction::UseSkill {
            skill: "example_skill".to_string(),
            input: json!({"query": "test input"}),
            mode: SkillMode::PromptOnly,
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
    };

    let pipeline = Pipeline {
        name: "test_pipeline".to_string(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline.clone());
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Skill execution should succeed");

    // Extract the captured LlmRequest from the mock provider
    let captured_request = mock_llm.captured_request.lock().unwrap();
    assert!(captured_request.is_some(), "MockLlmProvider should have captured an LLM request");
    
    let req = captured_request.as_ref().unwrap();
    let system_prompt = &req.system;

    // Verify that the system prompt contains the skill instructions
    assert!(
        system_prompt.contains("Complete the following tasks according to the examples"),
        "System prompt should contain skill instructions"
    );

    // Verify that the system prompt contains example 1
    assert!(
        system_prompt.contains("Example 1:"),
        "System prompt should contain 'Example 1:' marker"
    );
    assert!(
        system_prompt.contains("Simple example demonstrating task completion"),
        "System prompt should contain example 1 description"
    );
    assert!(
        system_prompt.contains("Successfully completed task1"),
        "System prompt should contain example 1 expected output"
    );

    // Verify that the system prompt contains example 2
    assert!(
        system_prompt.contains("Example 2:"),
        "System prompt should contain 'Example 2:' marker"
    );
    assert!(
        system_prompt.contains("Complex example with edge case handling"),
        "System prompt should contain example 2 description"
    );
    assert!(
        system_prompt.contains("Successfully completed task2"),
        "System prompt should contain example 2 expected output"
    );
}

/// Test that skill evaluation runs after execution and attaches eval_result to output.
/// Also verifies that min_score threshold comparison is wired in (not just formatted).
#[tokio::test]
async fn test_skill_eval_result_attached_and_min_score_wired() {
    let skill = Skill {
        name: "eval_skill".to_string(),
        description: "A skill with evaluation".to_string(),
        instructions: "Complete the task with high quality.".to_string(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: Some(SkillEval {
            criteria: vec![
                "quality".to_string(),
                "completeness".to_string(),
                "accuracy".to_string(),
            ],
            min_score: 0.75,  // 3 criteria, need at least 2.25 met (will need ~2-3 mentioned)
        }),
    };

    let mock_llm = MockLlmProvider::new(
        "Output demonstrates quality and accuracy and completeness in the solution."
    );
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm.clone())));
    
    let mut runner = PipelineRunner::new();
    runner.llm_client = Some(llm_client);

    let mut skill_registry = SkillRegistry::new();
    skill_registry.register(skill.clone());
    runner.skill_registry = Arc::new(skill_registry);

    let step = AgentStep {
        name: "eval_step".to_string(),
        guard_in: Guard::None,
        action: StepAction::UseSkill {
            skill: "eval_skill".to_string(),
            input: json!({}),
            mode: SkillMode::PromptOnly,
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
    };

    let pipeline = Pipeline {
        name: "test_pipeline".to_string(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline.clone());
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Skill execution should succeed");

    let run_result = result.unwrap();
    let step_result = run_result.step_results.get("eval_step");
    assert!(step_result.is_some(), "Step result should exist");

    let step_output = &step_result.unwrap().output;
    
    // Verify eval_result is attached
    assert!(
        step_output.eval_result.is_some(),
        "eval_result should be attached to StepOutput"
    );

    let eval_result = step_output.eval_result.as_ref().unwrap();
    
    // Verify the eval result format and content
    assert!(eval_result.contains("SkillEval[eval_skill]"), "Should contain skill name");
    assert!(eval_result.contains("criteria met"), "Should mention criteria matching");
    assert!(eval_result.contains("score:"), "Should include computed score");
    assert!(eval_result.contains("min: 0.75"), "Should include min_score threshold");

    // Verify that min_score comparison is GENUINE (not just formatted).
    // The output mentions "quality", "completeness", and "accuracy", so score should be 3/3 = 1.0.
    // Since 1.0 >= 0.75, there should NOT be "[BELOW threshold]"
    assert!(
        !eval_result.contains("[BELOW threshold]"),
        "Score 1.0 should NOT be below threshold 0.75"
    );
}

/// Test that when eval criteria are NOT met, min_score comparison marks it as BELOW threshold.
#[tokio::test]
async fn test_skill_eval_below_threshold_marked() {
    let skill = Skill {
        name: "low_score_skill".to_string(),
        description: "A skill that will score low".to_string(),
        instructions: "Try to complete this task.".to_string(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: Some(SkillEval {
            criteria: vec![
                "correct".to_string(),
                "efficient".to_string(),
                "readable".to_string(),
            ],
            min_score: 0.8,  // Need 2.4+ criteria met out of 3
        }),
    };

    // Output only mentions "correct", missing "efficient" and "readable"
    // Score will be 1/3 ≈ 0.33, which is below 0.8
    let mock_llm = MockLlmProvider::new("The solution is correct but not very efficient.");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm.clone())));
    
    let mut runner = PipelineRunner::new();
    runner.llm_client = Some(llm_client);

    let mut skill_registry = SkillRegistry::new();
    skill_registry.register(skill.clone());
    runner.skill_registry = Arc::new(skill_registry);

    let step = AgentStep {
        name: "low_score_step".to_string(),
        guard_in: Guard::None,
        action: StepAction::UseSkill {
            skill: "low_score_skill".to_string(),
            input: json!({}),
            mode: SkillMode::PromptOnly,
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
    };

    let pipeline = Pipeline {
        name: "test_pipeline".to_string(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline.clone());
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Skill execution should succeed");

    let run_result = result.unwrap();
    let step_result = run_result.step_results.get("low_score_step");
    assert!(step_result.is_some(), "Step result should exist");

    let eval_result = step_result.unwrap().output.eval_result.as_ref().unwrap();
    
    // Verify the score is below threshold and marked as such
    assert!(
        eval_result.contains("[BELOW threshold]"),
        "Score 0.33 should be marked as BELOW threshold 0.8, got: {}",
        eval_result
    );
}

/// Regression test: skills without examples or eval should work unchanged.
#[tokio::test]
async fn test_skill_without_examples_or_eval_unchanged() {
    let skill = Skill {
        name: "minimal_skill".to_string(),
        description: "A skill without examples or eval".to_string(),
        instructions: "Just complete the task".to_string(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],  // Empty examples
        eval: None,        // No eval
    };

    let mock_llm = MockLlmProvider::new("Task completed.");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm.clone())));
    
    let mut runner = PipelineRunner::new();
    runner.llm_client = Some(llm_client);

    let mut skill_registry = SkillRegistry::new();
    skill_registry.register(skill.clone());
    runner.skill_registry = Arc::new(skill_registry);

    let step = AgentStep {
        name: "minimal_step".to_string(),
        guard_in: Guard::None,
        action: StepAction::UseSkill {
            skill: "minimal_skill".to_string(),
            input: json!({}),
            mode: SkillMode::PromptOnly,
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
    };

    let pipeline = Pipeline {
        name: "test_pipeline".to_string(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline.clone());
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Minimal skill execution should succeed");

    let run_result = result.unwrap();
    let step_result = run_result.step_results.get("minimal_step");
    assert!(step_result.is_some(), "Step result should exist");

    let step_output = &step_result.unwrap().output;
    
    // Verify eval_result is NOT attached when there's no eval
    assert!(
        step_output.eval_result.is_none(),
        "eval_result should NOT be attached when skill has no eval"
    );
    
    // Verify output still works normally
    assert!(!step_output.raw.is_empty(), "Output should contain result from LLM");
}

/// Test that examples are properly formatted in the system prompt.
/// This is the key test that will FAIL if the examples-injection code is removed.
#[tokio::test]
async fn test_skill_prompt_injection_formats_examples_correctly() {
    let skill = Skill {
        name: "formatted_skill".to_string(),
        description: "Tests prompt formatting".to_string(),
        instructions: "Process according to these patterns:".to_string(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![
            SkillExample {
                input: json!({"text": "input1"}),
                expected_output: "output1".to_string(),
                description: "First pattern".to_string(),
            },
            SkillExample {
                input: json!({"text": "input2"}),
                expected_output: "output2".to_string(),
                description: "Second pattern".to_string(),
            },
            SkillExample {
                input: json!({"text": "input3"}),
                expected_output: "output3".to_string(),
                description: "Third pattern".to_string(),
            },
        ],
        eval: None,
    };

    let mock_llm = MockLlmProvider::new("Processing complete");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm.clone())));
    
    let mut runner = PipelineRunner::new();
    runner.llm_client = Some(llm_client);

    let mut skill_registry = SkillRegistry::new();
    skill_registry.register(skill.clone());
    runner.skill_registry = Arc::new(skill_registry);

    let step = AgentStep {
        name: "format_step".to_string(),
        guard_in: Guard::None,
        action: StepAction::UseSkill {
            skill: "formatted_skill".to_string(),
            input: json!({}),
            mode: SkillMode::PromptOnly,
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
    };

    let pipeline = Pipeline {
        name: "test_pipeline".to_string(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = dummy_agent("test_agent", pipeline.clone());
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Skill execution should succeed");

    let captured_request = mock_llm.captured_request.lock().unwrap();
    let req = captured_request.as_ref().unwrap();
    let system_prompt = &req.system;

    // Verify all three examples are in the prompt with correct formatting
    assert!(system_prompt.contains("Example 1:"), "Should have Example 1");
    assert!(system_prompt.contains("Example 2:"), "Should have Example 2");
    assert!(system_prompt.contains("Example 3:"), "Should have Example 3");

    // Verify input/output/description markers are present
    assert!(system_prompt.contains("Input:"), "Should have Input marker");
    assert!(system_prompt.contains("Expected Output:"), "Should have Expected Output marker");
    assert!(system_prompt.contains("Description:"), "Should have Description marker");

    // Verify actual content is in the prompt
    assert!(system_prompt.contains("First pattern"), "Should contain description 1");
    assert!(system_prompt.contains("Second pattern"), "Should contain description 2");
    assert!(system_prompt.contains("Third pattern"), "Should contain description 3");
    assert!(system_prompt.contains("output1"), "Should contain output 1");
    assert!(system_prompt.contains("output2"), "Should contain output 2");
    assert!(system_prompt.contains("output3"), "Should contain output 3");
}
