//! Integration tests: injection protection field propagation in parallel and sequential execution
//!
//! Tests that verify the injection_protection field is correctly copied from AgentStep definitions
//! into StepContext instances during both parallel and sequential execution paths.

use serde_json::json;
use verdict::prelude::*;
use std::sync::{Arc, Mutex};

mod common;

// ─── Test 3: Parallel injection protection field propagation ────────────────
#[tokio::test]
async fn test_parallel_injection_protection_field_propagates() {
    // This test verifies the fix in src/runner/parallel.rs line 72:
    //   step_ctx.injection_protection = step.injection_protection.clone();
    //
    // The fix ensures that when parallel steps are cloned into isolated contexts,
    // the injection_protection setting from the step definition is copied.
    // Without this assignment, step_ctx.injection_protection would default to None,
    // which would cause downstream code (like tool_use_loop.rs:317) to skip
    // sanitization even though Strict mode was configured.
    //
    // We prove this works by creating a Custom action that:
    // 1. Inspects ctx.injection_protection
    // 2. Logs it so we can observe what value it received
    // 3. Returns output that we can verify was processed correctly
    //
    // When parallel.rs line 72 is PRESENT (the fix): ctx.injection_protection = Strict
    // When parallel.rs line 72 is ABSENT (the bug): ctx.injection_protection = None (default)
    
    use verdict::prelude::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    
    // Capture what injection_protection value the context had
    let observed_protection = Arc::new(Mutex::new(None));
    let capture_clone = observed_protection.clone();
    
    // Create a Custom action that captures the injection_protection from its context
    let custom_fn = move |ctx: &StepContext| -> Result<StepOutput, StepError> {
        *capture_clone.lock().unwrap() = Some(ctx.injection_protection.clone());
        Ok(StepOutput::new("Custom action executed".into()))
    };
    
    // Create a pipeline with a parallel step that has Strict mode
    let pipeline = Pipeline {
        name: "parallel_injection_field_test".into(),
        steps: vec![
            AgentStep {
                name: "par_capture_protection".into(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(custom_fn)),
                guard_out: Guard::None,
                verdict: Verdict::None,
                tools: ToolSet::None,
                injection_protection: InjectionProtection::Strict,  // <-- KEY: step has Strict
                output_schema: None,
                dependencies: vec![],
                parallel: true,  // <-- KEY: runs in parallel
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
    // This proves parallel.rs line 72 (step_ctx.injection_protection = step.injection_protection.clone())
    // was executed and propagated the value correctly.
    //
    // If line 72 is ABSENT (the bug):
    //   - The Custom action receives ctx.injection_protection = None (the default)
    //   - This assertion FAILS because None != Strict
    //
    // If line 72 is PRESENT (the fix):
    //   - The Custom action receives ctx.injection_protection = Strict
    //   - This assertion PASSES
    let captured_val = captured.as_ref().unwrap();
    assert_eq!(
        *captured_val,
        InjectionProtection::Strict,
        "CRITICAL: The step's Strict mode MUST be propagated to ctx. Got: {:?}. \
         If you see None here, parallel.rs line 72 is missing or commented out.",
        captured_val
    );
    
    println!("✓ Test 3 PASSED: injection_protection was correctly propagated from step to context in parallel execution");
}

// ─── Test 4: SerializableStepContext round-trip preserves injection_protection (context.rs fix) ──

#[test]
fn test_context_serialization_preserves_injection_protection() {
    // This test constructs a REAL SerializableStepContext with injection_protection: Strict,
    // serializes it via serde_json, deserializes it, and verifies the field is genuinely preserved.
    //
    // The fix in context.rs is the `#[serde(default)]` attribute on the injection_protection field
    // in SerializableStepContext. This allows deserializing OLD snapshots (saved before the field existed)
    // without errors, while still preserving the new field in NEW snapshots.
    
    // Create a serializable context with Strict injection protection
    let mut serializable = SerializableStepContext {
        agent_name: "test_agent".into(),
        pipeline_name: "test_pipeline".into(),
        step_name: "test_step".into(),
        step_id: "test-id-12345".into(),
        request: json!({}),
        input: json!({}),
        output: None,
        step_results: Default::default(),
        delegation_depth: 0,
        parent_agent: None,
        active_skills: vec![],
        allowed_tools: ToolSet::None,
        trace: PipelineTrace::new(),
        budget: BudgetState::default(),
        conversation_history: MessageHistory::new(),
        filesystem_policy: FilesystemPolicy::new(std::path::PathBuf::from("/tmp")),
        network_policy: NetworkPolicy::DenyAll,
        agent_policy: AgentPolicy::default(),
        injection_protection: InjectionProtection::Strict,
        metadata: json!({}),
        request_context: RequestContext::default(),
    };
    
    // Verify it's set before serialization
    assert_eq!(
        serializable.injection_protection,
        InjectionProtection::Strict,
        "Serializable context should have Strict mode before serialization"
    );
    
    // Serialize to JSON
    let json_string = serde_json::to_string(&serializable)
        .expect("Should serialize SerializableStepContext");
    
    // Verify the serialized JSON contains "Strict"
    assert!(
        json_string.contains("Strict"),
        "Serialized JSON should contain Strict setting: {}",
        json_string
    );
    
    // Deserialize back from JSON
    let deserialized: SerializableStepContext = serde_json::from_str(&json_string)
        .expect("Should deserialize SerializableStepContext");
    
    // CRITICAL ASSERTION: The deserialized value MUST be Strict, not reset to None
    // This assertion FAILS if the context.rs fix is reverted (removing #[serde(default)])
    assert_eq!(
        deserialized.injection_protection,
        InjectionProtection::Strict,
        "Deserialization must preserve Strict setting"
    );
    
    // Also test with None to verify both values round-trip correctly
    serializable.injection_protection = InjectionProtection::None;
    let json_none = serde_json::to_string(&serializable)
        .expect("Should serialize with None");
    
    let deserialized_none: SerializableStepContext = serde_json::from_str(&json_none)
        .expect("Should deserialize with None");
    
    assert_eq!(
        deserialized_none.injection_protection,
        InjectionProtection::None,
        "None setting should round-trip correctly"
    );
    
    // CRITICAL TEST: Deserialize a JSON snapshot that LACKS the injection_protection field entirely
    // (simulating loading a snapshot saved BEFORE the fix was added to the codebase).
    // With #[serde(default)], this should succeed and default to None.
    // Without #[serde(default)], this would fail with a deserialization error.
    
    // Parse the serialized JSON, remove the injection_protection field, then deserialize
    let mut json_obj: serde_json::Map<String, serde_json::Value> = serde_json::from_str(&json_none)
        .expect("Should parse as JSON object");
    
    // Remove injection_protection to simulate an old snapshot
    json_obj.remove("injection_protection");
    
    let json_without_field = serde_json::Value::Object(json_obj).to_string();
    
    // WITHOUT #[serde(default)]: this would fail to deserialize
    // WITH #[serde(default)]: this should succeed and default to InjectionProtection::None
    let deserialized_old_snapshot: SerializableStepContext = serde_json::from_str(&json_without_field)
        .expect("Should deserialize old snapshot without injection_protection field (requires #[serde(default)])");
    
    assert_eq!(
        deserialized_old_snapshot.injection_protection,
        InjectionProtection::None,
        "Old snapshot without injection_protection field should default to None"
    );
    
    println!("✓ Test 4 PASSED: Serialization round-trip preserves injection_protection, and #[serde(default)] supports old snapshots");
}
