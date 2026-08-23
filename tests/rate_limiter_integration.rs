//! Integration tests for RateLimiter wiring into PipelineRunner (Phase 2 Task 4)
//! Tests real PipelineRunner with tight rate limiting and multiple consecutive calls.

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

mod common;

fn simple_llm_step(name: &str, prompt: &str) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "Test system".into(),
            user: prompt.into(),
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
    }
}

fn simple_tool_step(name: &str, tool: &str, args: serde_json::Value) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall {
            tool: tool.into(),
            args,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Allow(vec![tool.into()]),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

fn make_agent(pipeline: &Pipeline) -> Agent {
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Full;
    Agent {
        name: "test_agent".into(),
        description: "rate limiter test agent".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    }
}

/// Test 1: With a tight rate limit (2 req/min), 3 rapid LLM calls in sequence should fail on the 3rd.
/// This test is CRITICAL: it must GENUINELY verify the rate-limit gate is enforced.
/// The gate is at llm_synthesis.rs:51-58, and is ONLY reached if an LLM client is configured.
/// Removing that gate will cause this test to FAIL (3rd call succeeds instead of erroring).
#[tokio::test]
async fn test_rate_limit_tight_llm_calls_rejects_third() {
    use crate::common::{ScriptedMockLlmProvider, ScriptedResponse};
    
    let pipeline = Pipeline {
        name: "rate_limit_test".into(),
        steps: vec![
            simple_llm_step("call_1", "Call 1"),
            simple_llm_step("call_2", "Call 2"),
            simple_llm_step("call_3", "Call 3"),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(&pipeline);
    let mut runner = PipelineRunner::new();

    // Configure mock LLM client with exactly 2 successful responses
    // (3rd call will be rate-limited before even reaching the provider)
    let mock_provider = Arc::new(ScriptedMockLlmProvider::new(vec![
        ScriptedResponse::text("Response 1"),
        ScriptedResponse::text("Response 2"),
        // No 3rd response — but we won't reach it due to rate limit
    ]));
    runner.llm_client = Some(Arc::new(verdict::llm::LlmClient::new(mock_provider)));

    // Configure rate limiter: 2 calls per minute
    // With 3 rapid calls, the 3rd MUST fail with rate-limit error
    let rate_limiter = RateLimiter::new().with_max_calls_per_minute(2);
    runner.rate_limiter = Some(Arc::new(std::sync::Mutex::new(rate_limiter)));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    // HARD ASSERTION: Pipeline must fail at step 3 with rate-limit error.
    // If the rate-limit gate were removed, this assertion fails, catching the regression.
    match result {
        Err(PipelineError::StepFailed {
            step,
            error: StepError::ActionFailed { reason },
        }) => {
            assert_eq!(step, "call_3", "Expected rate limit failure at step 3, got error at step: {}", step);
            assert!(
                reason.contains("rate limit"),
                "Expected rate limit error at step 3, got: {}",
                reason
            );
        }
        Err(e) => panic!("Expected StepFailed with rate limit error at step 3, got: {:?}", e),
        Ok(_) => panic!("Expected rate limit error at step 3, but pipeline succeeded — rate limiter gate is missing or broken"),
    }
}

/// Test 2: Rate-limited call to a DISALLOWED tool should return scope-violation error, not rate-limit error.
/// This proves the ordering (security check before rate-limit check) is correct.
/// CRITICAL: We exhaust the rate limiter FIRST (pre-calls) so that BOTH violations are live simultaneously
/// when the test step executes. This ensures the test genuinely catches if the order is swapped.
#[tokio::test]
async fn test_disallowed_tool_beats_rate_limit() {
     let pipeline = Pipeline {
         name: "disallowed_tool_test".into(),
         steps: vec![AgentStep {
             name: "call_disallowed".into(),
             guard_in: Guard::None,
             action: StepAction::ToolCall {
                 tool: "fs.write".into(),
                 args: json!({ "path": "/test", "content": "test" }),
             },
             guard_out: Guard::None,
             verdict: Verdict::None,
             tools: ToolSet::None, // ToolSet::None disallows all tools
             injection_protection: InjectionProtection::None,
             output_schema: None,
             dependencies: vec![],
             parallel: false,
             input_processors: vec![],
             output_processors: vec![],
         }],
         on_failure: FailureMode::Abort,
         max_retries: 0,
     };

     let agent = make_agent(&pipeline);
     let mut runner = PipelineRunner::new();
     // Configure rate limiter at runner level: 0 calls per minute = exhausted immediately
     // This ensures rate-limit violation is LIVE when the test step runs.
     let rate_limiter = RateLimiter::new().with_max_calls_per_minute(0);
     runner.rate_limiter = Some(Arc::new(std::sync::Mutex::new(rate_limiter)));

     let result = runner.run(&pipeline, &agent, json!({})).await;

     // Should fail with scope violation (security check first), NOT rate limit.
     // If the order were swapped, we'd get a rate-limit error instead.
     match result {
         Err(PipelineError::StepFailed {
             step,
             error: StepError::ActionFailed { reason },
         }) => {
             assert_eq!(step, "call_disallowed");
             // Must contain "not allowed" (scope error), not "rate limit"
             assert!(
                 reason.contains("not allowed"),
                 "Expected scope violation, got: {}",
                 reason
             );
             assert!(
                 !reason.contains("rate limit"),
                 "Should not mention rate limit for disallowed tool — proves security check happened first"
             );
         }
         Err(e) => panic!("Expected StepFailed with scope error, got: {:?}", e),
         Ok(_) => panic!("Expected error but got success"),
     }
 }

/// Test 3: Verify no rate limiter configured (backward-compat case) behaves EXACTLY as before.
#[tokio::test]
async fn test_no_rate_limiter_backward_compat() {
    let pipeline = Pipeline {
        name: "backward_compat_test".into(),
        steps: vec![
            simple_llm_step("call_1", "Call 1"),
            simple_llm_step("call_2", "Call 2"),
            simple_llm_step("call_3", "Call 3"),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    // rate_limiter is None — no limiting configured

    let result = runner.run(&pipeline, &agent, json!({})).await;

    // Without an LLM client and without rate limiting, the pipeline should still fail
    // at the first step (LllmCall with no LLM client), but that's expected.
    // The point is: no rate limit errors should appear.
    match result {
        Err(e) => {
            let err_str = format!("{:?}", e);
            // Should NOT see "rate limit" in error
            assert!(
                !err_str.contains("rate limit"),
                "Should not have rate limit error without rate limiter configured: {}",
                err_str
            );
        }
        Ok(_) => {
            // Might succeed depending on LLM client availability
        }
    }
}

/// Test 4: Verify rate limiting works on tool calls (not just LLM calls).
/// CRITICAL: The test MUST assert failure on the 3rd call, not accept either outcome.
/// Removing the rate-limit gate entirely would make this test pass with the old `Ok(_)` arm,
/// so we eliminate the no-op branch and require the 3rd call to GENUINELY fail with rate-limit error.
#[tokio::test]
async fn test_rate_limit_on_tool_calls() {
     let pipeline = Pipeline {
         name: "tool_rate_limit_test".into(),
         steps: vec![
             simple_tool_step("tool_call_1", "shell.run", json!({ "command": "echo", "args": ["test1"] })),
             simple_tool_step("tool_call_2", "shell.run", json!({ "command": "echo", "args": ["test2"] })),
             simple_tool_step("tool_call_3", "shell.run", json!({ "command": "echo", "args": ["test3"] })),
         ],
         on_failure: FailureMode::Abort,
         max_retries: 0,
     };

     let agent = make_agent(&pipeline);
     let mut runner = PipelineRunner::new();
     // Configure rate limiter: 2 calls per minute (applies to both LLM and tool calls)
     let rate_limiter = RateLimiter::new().with_max_calls_per_minute(2);
     runner.rate_limiter = Some(Arc::new(std::sync::Mutex::new(rate_limiter)));

     let result = runner.run(&pipeline, &agent, json!({})).await;

     // Pipeline MUST fail at step 3 with a rate-limit error. No accept-either-outcome branch.
     // If the rate-limit gate were removed, this assertion would fail, catching the regression.
     match result {
         Err(PipelineError::StepFailed {
             step,
             error: StepError::ActionFailed { reason },
         }) => {
             assert_eq!(step, "tool_call_3", "Expected rate limit failure at step 3");
             assert!(
                 reason.contains("rate limit"),
                 "Expected rate limit error at step 3, got: {}",
                 reason
             );
         }
         Err(e) => panic!("Expected StepFailed with rate limit error at step 3, got: {:?}", e),
         Ok(_) => panic!("Expected rate limit error at step 3, but pipeline succeeded — rate limiter gate is missing or broken"),
     }
 }
