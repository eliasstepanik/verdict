//! Integration tests: ToolUseLoop redaction gate behavior (strict vs. none mode)
//!
//! Tests that verify the injection_protection field in ToolUseLoop steps controls
//! whether secrets are redacted from tool results before appending to message history.

use serde_json::json;
use verdict::prelude::*;
use verdict::registry::ToolRegistry;
use std::sync::Arc;

mod common;
use common::{ScriptedMockLlmProvider, ScriptedResponse, SecretBearingTestTool};

// ─── Helper: Common test harness for ToolUseLoop tests ────────────────────
/// Sets up a minimal pipeline with a ToolUseLoop step and runs it with the given script.
/// Returns captured LLM requests for assertions.
async fn run_tool_loop_test(
    script: Vec<ScriptedResponse>,
    injection_protection: InjectionProtection,
    step_name: &str,
    pipeline_name: &str,
) -> Vec<LlmRequest> {
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let captured_requests = mock_provider.captured_requests.clone();
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(SecretBearingTestTool::new());
    
    let tool_loop_step = AgentStep {
        name: step_name.into(),
        guard_in: Guard::None,
        action: StepAction::ToolUseLoop {
            system: "You are a helpful assistant.".into(),
            user: "Call the test tool.".into(),
            model: ProviderSpec {
                model: "mock".into(),
                provider: "mock".into(),
            },
            tools: vec!["test_tool".into()],
            max_rounds: 2,
            stop_condition: StopCondition::TextOnly,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    
    let pipeline = Pipeline {
        name: pipeline_name.into(),
        steps: vec![tool_loop_step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Full;
    
    let agent = Agent {
        name: "test_agent".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    };
    
    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));
    
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Pipeline should complete successfully. Got: {:?}", result);
    
    let requests = captured_requests.lock().unwrap().clone();
    requests
}

// ─── Test 5: ToolUseLoop gate redacts secrets with Strict mode ──────────────
#[tokio::test]
async fn test_tool_use_loop_redacts_secrets_with_strict_mode() {
    let script = vec![
        ScriptedResponse::tool_call("test_tool", json!({ "arg": "test" })),
        ScriptedResponse::text("Tool result processed."),
    ];
    
    let requests = run_tool_loop_test(
        script,
        InjectionProtection::Strict,
        "strict_mode_tool_loop",
        "strict_mode_secret_test",
    ).await;
    
    assert!(requests.len() >= 2, "Expected at least 2 LLM calls, got {}", requests.len());
    
    let history_text = format!("{:?}", requests[1].history);
    let raw_secret = "sk-proj-fake1234567890abcdefghij";
    
    assert!(
        !history_text.contains(raw_secret),
        "Raw secret found in Strict mode history!"
    );
    assert!(
        history_text.contains("[REDACTED"),
        "Redaction marker not found in history!"
    );
    
    println!("✓ Test 5 PASSED: ToolUseLoop with Strict mode GENUINELY redacted the secret");
}

// ─── Test 6: ToolUseLoop gate allows secrets with None mode ──────────────
#[tokio::test]
async fn test_tool_use_loop_allows_secrets_with_none_mode() {
    let script = vec![
        ScriptedResponse::tool_call("test_tool", json!({ "arg": "test" })),
        ScriptedResponse::text("Tool result processed."),
    ];
    
    let requests = run_tool_loop_test(
        script,
        InjectionProtection::None,
        "none_mode_tool_loop",
        "none_mode_secret_test",
    ).await;
    
    assert!(requests.len() >= 2, "Expected at least 2 LLM calls, got {}", requests.len());
    
    let history_text = format!("{:?}", requests[1].history);
    let raw_secret = "sk-proj-fake1234567890abcdefghij";
    
    assert!(
        history_text.contains(raw_secret),
        "Raw secret should be present in None mode history!"
    );
    
    println!("✓ Test 6 PASSED: ToolUseLoop with None mode allows raw secret to pass through");
}

// ─── Test 7: ToolUseLoop XML format redacts secrets with Strict mode ───────
#[tokio::test]
async fn test_tool_use_loop_redacts_secrets_with_strict_mode_xml() {
    let script = vec![
        ScriptedResponse::text(
            "<invoke name=\"test_tool\">\
             <parameter name=\"arg\">test</parameter>\
             </invoke>"
        ),
        ScriptedResponse::text("Tool result processed."),
    ];
    
    let requests = run_tool_loop_test(
        script,
        InjectionProtection::Strict,
        "strict_mode_tool_loop_xml",
        "strict_mode_secret_test_xml",
    ).await;
    
    assert!(requests.len() >= 2, "Expected at least 2 LLM calls, got {}", requests.len());
    
    let history_text = format!("{:?}", requests[1].history);
    let raw_secret = "sk-proj-fake1234567890abcdefghij";
    
    assert!(
        !history_text.contains(raw_secret),
        "Raw secret found in XML Strict mode history!"
    );
    assert!(
        history_text.contains("[REDACTED"),
        "Redaction marker not found in history!"
    );
    
    println!("✓ Test 7 PASSED: ToolUseLoop with Strict mode GENUINELY redacted the secret in XML path");
}
