//! Integration tests: injection_protection field propagation in sequential execution path
//!
//! Tests that verify the injection_protection field is correctly copied from AgentStep definitions
//! into StepContext instances during sequential (non-parallel) execution paths.

use serde_json::json;
use verdict::prelude::*;
use std::sync::{Arc, Mutex};

mod common;

// ─── Test 5: Sequential (default) path's injection_protection assignment (D3) ──────────────────
//
// This test verifies the fix in src/runner/execution.rs line 392:
//   ctx.injection_protection = step.injection_protection.clone();
//
// The SEQUENTIAL execution path (parallel: false, the default) MUST propagate the step's
// injection_protection setting to the context, just as the parallel path does in parallel.rs line 72.
//
// Without line 392, the sequential path's context would have injection_protection = None (default),
// which breaks secret sanitization in tool_use_loop.rs:317 for sequential steps with Strict mode.

#[tokio::test]
async fn test_sequential_injection_protection_field_propagates() {
    
    // Capture what injection_protection value the context had
    let observed_protection = Arc::new(Mutex::new(None));
    let capture_clone = observed_protection.clone();
    
    // Create a Custom action that captures the injection_protection from its context
    let custom_fn = move |ctx: &StepContext| -> Result<StepOutput, StepError> {
        *capture_clone.lock().unwrap() = Some(ctx.injection_protection.clone());
        Ok(StepOutput::new("Custom action executed".into()))
    };
    
    // Create a pipeline with a SEQUENTIAL step (parallel: false, the DEFAULT) that has Strict mode
    let pipeline = Pipeline {
        name: "sequential_injection_field_test".into(),
        steps: vec![
            AgentStep {
                name: "seq_capture_protection".into(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(custom_fn)),
                guard_out: Guard::None,
                verdict: Verdict::None,
                tools: ToolSet::None,
                injection_protection: InjectionProtection::Strict,  // <-- KEY: step has Strict
                output_schema: None,
                dependencies: vec![],
                parallel: false,  // <-- KEY: DEFAULT, runs SEQUENTIALLY
                input_processors: vec![],
                output_processors: vec![],
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    
    // Create an agent with permissive policy
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Full;
    
    let agent = Agent {
        name: "test_agent".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    };
    
    // Run the pipeline
    let result = PipelineRunner::new()
        .run(&pipeline, &agent, json!({}))
        .await;
    
    assert!(
        result.is_ok(),
        "Pipeline should complete successfully. Got: {:?}",
        result
    );
    
    // Verify the custom action was invoked and captured the protection setting
    let captured = observed_protection.lock().unwrap();
    assert!(
        captured.is_some(),
        "Custom action should have been invoked and captured injection_protection"
    );
    
    // THE CRITICAL ASSERTION: The context's injection_protection MUST be Strict
    // This proves execution.rs line 392 (ctx.injection_protection = step.injection_protection.clone())
    // was executed and propagated the value correctly in the SEQUENTIAL path.
    //
    // If line 392 is ABSENT (the bug):
    //   - The Custom action receives ctx.injection_protection = None (the default)
    //   - This assertion FAILS because None != Strict
    //
    // If line 392 is PRESENT (the fix):
    //   - The Custom action receives ctx.injection_protection = Strict
    //   - This assertion PASSES
    let captured_val = captured.as_ref().unwrap();
    assert_eq!(
        *captured_val,
        InjectionProtection::Strict,
        "CRITICAL: The step's Strict mode MUST be propagated to ctx in SEQUENTIAL path. Got: {:?}. \
         If you see None here, execution.rs line 392 is missing or commented out.",
        captured_val
    );
    
    println!("✓ Test 5 PASSED: injection_protection was correctly propagated from step to context in SEQUENTIAL execution (default parallel: false)");
}
