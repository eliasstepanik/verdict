//! Integration tests: Synthesis path redaction
//!
//! Tests that verify secret sanitization works in the synthesis path
//! (ToolUseLoop exits via max_rounds without text answer):
//! - Synthesis path in JSON context
//! - Synthesis path in XML context

use serde_json::json;
use verdict::prelude::*;
use verdict::registry::ToolRegistry;
use std::sync::Arc;

mod common;
use common::SecretBearingTestTool;

// ─── Test 9: Synthesis path redacts secrets (JSON format) with Strict mode ──────────────
//
// CRITICAL TEST: This verifies that the shared gate_tool_result() function
// protects the SYNTHESIS PATH (tool_use_loop_synthesis.rs lines 124 & 238)
// when a ToolUseLoop exits via max_rounds without a text answer.
//
// Synthesis path flow:
// 1. ToolUseLoop runs for max_rounds, each round LLM returns a tool call
// 2. On final round, tool result is prepared and the loop exits
// 3. Synthesis path calls llm_client.complete(...) with the tool result in history
// 4. The shared gate_tool_result() at tool_use_loop_synthesis.rs MUST redact before appending

#[tokio::test]
async fn test_synthesis_path_redacts_secrets_json_with_strict_mode() {
    use common::{ScriptedMockLlmProvider, ScriptedResponse};
    
    // Script: LLM makes tool calls for max_rounds, then synthesis calls the LLM again
    // Round 1 (tool call 1), Round 2 (tool call 2, hits max_rounds), then synthesis round
    let script = vec![
        // Round 1: LLM returns JSON-style tool call
        ScriptedResponse::tool_call("test_tool", json!({ "arg": "test1" })),
        // Round 2: LLM returns another JSON tool call (hits max_rounds = 2)
        // At this point, ToolUseLoop will not continue the loop; instead it will:
        // 1. Execute this tool call
        // 2. Prepare the tool result for synthesis (this is where gate_tool_result must apply redaction)
        // 3. Call synthesis path to generate final text
        ScriptedResponse::tool_call("test_tool", json!({ "arg": "test2" })),
        // Round 3: Synthesis path receives the redacted tool result and synthesizes text
        ScriptedResponse::text("Final answer generated from tool result."),
    ];
    
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let captured_requests = mock_provider.captured_requests.clone();
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    
    // Create a tool registry with test_tool
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(SecretBearingTestTool::new());
    
    // Create a ToolUseLoop step with Strict mode and max_rounds = 2
    // This will force the synthesis path to execute
    let tool_loop_step = AgentStep {
        name: "strict_mode_synthesis_json".into(),
        guard_in: Guard::None,
        action: StepAction::ToolUseLoop {
            system: "You are a helpful assistant.".into(),
            user: "Call the test tool twice.".into(),
            model: ProviderSpec {
                model: "mock".into(),
                provider: "mock".into(),
            },
            tools: vec!["test_tool".into()],
            max_rounds: 2,  // Forces synthesis path after 2 rounds
            stop_condition: StopCondition::TextOnly,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::Strict,  // <-- KEY: Strict mode
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    
    let pipeline = Pipeline {
        name: "strict_mode_synthesis_json_test".into(),
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
    
    // Run the pipeline
    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));
    
    let result = runner.run(&pipeline, &agent, json!({})).await;
    
    assert!(
        result.is_ok(),
        "Pipeline should complete successfully. Got: {:?}",
        result
    );
    
    // CRITICAL ASSERTION: Inspect the third LLM call (synthesis path call)
    // This call receives the tool result in its history, and the gate_tool_result()
    // function in tool_use_loop_synthesis.rs MUST have redacted it
    let requests = captured_requests.lock().unwrap();
    assert!(
        requests.len() >= 3,
        "Expected at least 3 LLM calls (round 1: tool call, round 2: tool call, round 3: synthesis). Got {}",
        requests.len()
    );
    
    // The third request (index 2) is the synthesis path call
    let synthesis_request = &requests[2];
    let history_text = format!("{:?}", synthesis_request.history);
    
    // THE ACTUAL PROOF OF SYNTHESIS PATH REDACTION (JSON):
    // With Strict mode and synthesis path active, gate_tool_result() at
    // tool_use_loop_synthesis.rs:238 MUST have redacted the secret
    let raw_secret = "sk-proj-fake1234567890abcdefghij";
    assert!(
        !history_text.contains(raw_secret),
        "CRITICAL FAILURE: Raw secret '{}' found in synthesis path LLM call's history in Strict mode (JSON)! \
         This means gate_tool_result() at tool_use_loop_synthesis.rs:238 did NOT work. History: {}",
        raw_secret,
        history_text
    );
    
    // Also verify the [REDACTED] marker is present (proof gate_tool_result actually ran)
    assert!(
        history_text.contains("[REDACTED"),
        "Expected redaction marker [REDACTED] in synthesis path history (JSON), but not found. History: {}",
        history_text
    );
    
    println!("✓ Test 9 PASSED: Synthesis path with Strict mode GENUINELY redacted the secret in JSON format (verified via synthesis LLM call history)");
}

// ─── Test 10: Synthesis path redacts secrets (XML format) with Strict mode ────────────────
//
// CRITICAL TEST: Same as Test 9, but verifies the XML-branch synthesis path
// gate_tool_result() at tool_use_loop_synthesis.rs:124 (XML branch) works identically.

#[tokio::test]
async fn test_synthesis_path_redacts_secrets_xml_with_strict_mode() {
    use common::{ScriptedMockLlmProvider, ScriptedResponse};
    
    // Script: LLM makes tool calls (XML format) for max_rounds, then synthesis calls the LLM again
    let script = vec![
        // Round 1: LLM returns XML-style tool call
        ScriptedResponse::text(
            "<invoke name=\"test_tool\">\
             <parameter name=\"arg\">test1</parameter>\
             </invoke>"
        ),
        // Round 2: LLM returns another XML tool call (hits max_rounds = 2)
        // At this point, ToolUseLoop will not continue the loop; instead it will:
        // 1. Execute this tool call
        // 2. Prepare the tool result for synthesis
        // 3. Call synthesis path to generate final text
        ScriptedResponse::text(
            "<invoke name=\"test_tool\">\
             <parameter name=\"arg\">test2</parameter>\
             </invoke>"
        ),
        // Round 3: Synthesis path receives the redacted tool result and synthesizes text
        ScriptedResponse::text("Final answer generated from tool result."),
    ];
    
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let captured_requests = mock_provider.captured_requests.clone();
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    
    // Create a tool registry with test_tool
    let mut tool_registry = ToolRegistry::new();
    tool_registry.register(SecretBearingTestTool::new());
    
    // Create a ToolUseLoop step with Strict mode and max_rounds = 2
    let tool_loop_step = AgentStep {
        name: "strict_mode_synthesis_xml".into(),
        guard_in: Guard::None,
        action: StepAction::ToolUseLoop {
            system: "You are a helpful assistant.".into(),
            user: "Call the test tool twice using XML format.".into(),
            model: ProviderSpec {
                model: "mock".into(),
                provider: "mock".into(),
            },
            tools: vec!["test_tool".into()],
            max_rounds: 2,  // Forces synthesis path after 2 rounds
            stop_condition: StopCondition::TextOnly,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::Strict,  // <-- KEY: Strict mode
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    
    let pipeline = Pipeline {
        name: "strict_mode_synthesis_xml_test".into(),
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
    
    // Run the pipeline
    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));
    
    let result = runner.run(&pipeline, &agent, json!({})).await;
    
    assert!(
        result.is_ok(),
        "Pipeline should complete successfully. Got: {:?}",
        result
    );
    
    // CRITICAL ASSERTION: Inspect the third LLM call (synthesis path call)
    let requests = captured_requests.lock().unwrap();
    assert!(
        requests.len() >= 3,
        "Expected at least 3 LLM calls (round 1: tool call, round 2: tool call, round 3: synthesis). Got {}",
        requests.len()
    );
    
    // The third request (index 2) is the synthesis path call
    let synthesis_request = &requests[2];
    let history_text = format!("{:?}", synthesis_request.history);
    
    // THE ACTUAL PROOF OF SYNTHESIS PATH REDACTION (XML):
    // With Strict mode and synthesis path active, gate_tool_result() at
    // tool_use_loop_synthesis.rs:124 (XML branch) MUST have redacted the secret
    let raw_secret = "sk-proj-fake1234567890abcdefghij";
    assert!(
        !history_text.contains(raw_secret),
        "CRITICAL FAILURE: Raw secret '{}' found in synthesis path LLM call's history in Strict mode (XML)! \
         This means gate_tool_result() at tool_use_loop_synthesis.rs:124 did NOT work. History: {}",
        raw_secret,
        history_text
    );
    
    // Also verify the [REDACTED] marker is present (proof gate_tool_result actually ran)
    assert!(
        history_text.contains("[REDACTED"),
        "Expected redaction marker [REDACTED] in synthesis path history (XML), but not found. History: {}",
        history_text
    );
    
    println!("✓ Test 10 PASSED: Synthesis path with Strict mode GENUINELY redacted the secret in XML format (verified via synthesis LLM call history)");
}
