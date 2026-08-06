//! Integration tests: LLM calls using the real llm-proxy provider
//!
//! These tests make live HTTP calls to:
//!   Base URL: http://192.168.178.166:4141/v1
//!   API Key:  sk-llmp-239b82f7192fd75bff9300d1391bacafe049144a19f84d6d26f9cbc5cfb944d9.opencode
//!   Model:    claude-haiku-4-5-20251001
//!
//! Each test sets the required env vars itself and calls LlmClient::from_env().
//!
//! Run all: `cargo test --test integration_llm_calls`
//! Run one: `cargo test --test integration_llm_calls test_llm_call_produces_nonempty_output`

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

// ─── constants ────────────────────────────────────────────────────────────────

const PROXY_BASE_URL: &str = "http://192.168.178.166:4141/v1";
const PROXY_API_KEY: &str =
    "sk-llmp-239b82f7192fd75bff9300d1391bacafe049144a19f84d6d26f9cbc5cfb944d9.opencode";
const PROXY_MODEL: &str = "claude-haiku-4-5-20251001";

fn setup_env() {
    std::env::set_var("OPENAI_BASE_URL", PROXY_BASE_URL);
    std::env::set_var("OPENAI_API_KEY", PROXY_API_KEY);
    std::env::set_var("OPENAI_MODEL", PROXY_MODEL);
}

fn llm_client() -> Arc<LlmClient> {
    setup_env();
    Arc::new(LlmClient::from_env().expect("LlmClient::from_env must succeed with proxy env vars"))
}

fn simple_agent(pipeline: &Pipeline) -> Agent {
    Agent {
        name: "test_agent".into(),
        description: "llm integration test agent".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: Vec::new(),
    }
}

// ─── Test 1: LlmCall step produces non-empty output ──────────────────────────

#[tokio::test]
async fn test_llm_call_produces_nonempty_output() {
    let step = AgentStep {
        name: "ask".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a helpful assistant. Reply with exactly one short sentence.".into(),
            user: "Say hello.".into(),
            model: None,
            conversation_id: None,
            append_to_history: false,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "llm_hello".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_llm_client(llm_client());

    let result = runner
        .run(&pipeline, &agent, json!({}))
        .await
        .expect("LLM call should succeed with proxy");

    assert!(result.success);
    let output = &result.step_results["ask"].output.raw;
    assert!(!output.is_empty(), "LLM output must be non-empty");
    assert!(
        output.len() > 5,
        "LLM output should be a real response, got: {output}"
    );
}

// ─── Test 2: LlmCall without client fails with ActionFailed ──────────────────

#[tokio::test]
async fn test_llm_call_without_client_fails_gracefully() {
    let step = AgentStep {
        name: "ask".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "Test".into(),
            user: "Say hi.".into(),
            model: None,
            conversation_id: None,
            append_to_history: false,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "no_llm".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    // No LLM client set on runner
    let mut runner = PipelineRunner::new();
    let err = runner.run(&pipeline, &agent, json!({})).await.unwrap_err();

    match err {
        PipelineError::StepFailed { step, error } => {
            assert_eq!(step, "ask");
            let msg = error.to_string();
            assert!(
                msg.contains("LLM") || msg.contains("client") || msg.contains("not configured"),
                "error must explain missing LLM client: {msg}"
            );
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
}

// ─── Test 3: Template substitution {step_name} resolves in LlmCall ────────────

#[tokio::test]
async fn test_llm_call_template_substitution_resolves_prior_step_output() {
    let step1 = AgentStep {
        name: "compute".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| Ok(StepOutput::new("the number 42".into())))),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    // {compute} in the user prompt should be replaced with "the number 42"
    let step2 = AgentStep {
        name: "respond".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a helpful assistant. Reply with exactly one short sentence.".into(),
            user: "The prior step computed: {compute}. Just say OK and repeat the value back."
                .into(),
            model: None,
            conversation_id: None,
            append_to_history: false,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "template_test".into(),
        steps: vec![step1, step2],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_llm_client(llm_client());

    let result = runner
        .run(&pipeline, &agent, json!({}))
        .await
        .expect("pipeline with template substitution should succeed");

    assert!(result.success);
    let output = &result.step_results["respond"].output.raw;
    assert!(!output.is_empty(), "LLM response must not be empty");
    // The LLM should have received "the number 42" in its prompt and echoed it back
    assert!(
        output.to_lowercase().contains("42") || output.to_lowercase().contains("number"),
        "LLM response should contain '42' or 'number' (template resolved): {output}"
    );
}

// ─── Test 4: Multi-turn conversation history is appended ─────────────────────

#[tokio::test]
async fn test_llm_multi_turn_conversation_history_appended() {
    let conv_id = "integration-test-conv-001";

    let step1 = AgentStep {
        name: "first_turn".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a helpful assistant. Remember what the user tells you.".into(),
            user: "My favourite colour is blue. Just say 'Got it'.".into(),
            model: None,
            conversation_id: Some(conv_id.into()),
            append_to_history: true,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let step2 = AgentStep {
        name: "second_turn".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a helpful assistant. Remember what the user tells you.".into(),
            user: "What is my favourite colour? Answer in one word.".into(),
            model: None,
            conversation_id: Some(conv_id.into()),
            append_to_history: true,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "multi_turn".into(),
        steps: vec![step1, step2],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_llm_client(llm_client());

    let result = runner
        .run(&pipeline, &agent, json!({}))
        .await
        .expect("multi-turn pipeline should succeed");

    assert!(result.success);
    let first = &result.step_results["first_turn"].output.raw;
    let second = &result.step_results["second_turn"].output.raw;
    assert!(!first.is_empty(), "first turn must have non-empty output");
    // Second turn should recall the colour from conversation history
    assert!(
        second.to_lowercase().contains("blue"),
        "LLM must recall colour 'blue' from conversation history: {second}"
    );
}

// ─── Test 5: LlmJudge verdict passes when output matches pattern ──────────────

#[tokio::test]
async fn test_llm_judge_verdict_passes_when_pattern_present() {
    let step = AgentStep {
        name: "judged".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| {
            Ok(StepOutput::new("The capital of France is Paris.".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::LlmJudge {
            system: "You are a fact checker. Evaluate if the output is factually correct. Respond with exactly PASS or FAIL.".into(),
            input_template: "Output to evaluate: {output}".into(),
            model: None,
            pass_on_pattern: "PASS".into(),
        },
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "llm_judge".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_llm_client(llm_client());

    let result = runner
        .run(&pipeline, &agent, json!({}))
        .await
        .expect("LlmJudge on correct fact should pass");

    assert!(result.success);
    assert!(result.step_results["judged"].verdict_passed);
}

// ─── Test 6: LlmJudge verdict fails when pattern not present ─────────────────

#[tokio::test]
async fn test_llm_judge_verdict_fails_when_pattern_absent() {
    let step = AgentStep {
        name: "judged".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| {
            // Clearly wrong fact that the LLM should reject
            Ok(StepOutput::new("The capital of France is London.".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::LlmJudge {
            system: "You are a fact checker. Evaluate if the output is factually correct. Respond with exactly PASS if correct, or FAIL if incorrect.".into(),
            input_template: "Output to evaluate: {output}".into(),
            model: None,
            pass_on_pattern: "PASS".into(),
        },
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "llm_judge_fail".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_llm_client(llm_client());

    let err = runner.run(&pipeline, &agent, json!({})).await.unwrap_err();

    assert!(
        matches!(err, PipelineError::VerdictFailed { .. }),
        "LlmJudge should fail on incorrect fact, got: {err:?}"
    );
}

// ─── Test 7: SemanticCheck guard passes on semantically correct output ─────────

#[tokio::test]
async fn test_semantic_check_guard_passes_on_correct_output() {
    let step = AgentStep {
        name: "semantic".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| {
            Ok(StepOutput::new("The sum of 2 + 2 is 4.".into()))
        })),
        guard_out: Guard::SemanticCheck("The output must state that 2 + 2 equals 4".into()),
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "semantic_pass".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_llm_client(llm_client());

    let result = runner
        .run(&pipeline, &agent, json!({}))
        .await
        .expect("SemanticCheck should pass when output satisfies the requirement");

    assert!(result.success);
    assert!(result.step_results["semantic"].verdict_passed);
}

// ─── Test 8: SemanticCheck guard fails on semantically wrong output ────────────

#[tokio::test]
async fn test_semantic_check_guard_fails_on_wrong_output() {
    let step = AgentStep {
        name: "semantic_fail".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| {
            Ok(StepOutput::new(
                "The sky is green and grass is blue.".into(),
            ))
        })),
        guard_out: Guard::SemanticCheck(
            "The output must correctly state natural colour facts (blue sky, green grass)".into(),
        ),
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "semantic_fail".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_llm_client(llm_client());

    let err = runner.run(&pipeline, &agent, json!({})).await.unwrap_err();

    assert!(
        matches!(
            err,
            PipelineError::GuardFailed {
                phase: GuardPhase::Out,
                ..
            }
        ),
        "SemanticCheck should fail on incorrect output, got: {err:?}"
    );
}
