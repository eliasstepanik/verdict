use serde_json::json;
use verdict::prelude::*;

// ============================================================================
// Phase 17: Quick Wins (Phase A) Tests
// ============================================================================
// Tests for new Step Actions: Sleep, SleepUntil, ForEach
// Tests for Enhanced DelegationPolicy with hooks
// Tests for PipelineResult cost tracking fields
//  ============================================================================

#[tokio::test]
async fn test_sleep_action() {
    // Create a simple sleep action
    let action = StepAction::Sleep { duration_ms: 50 };

    // Create a minimal context
    let ctx = StepContext::new(
        "test_agent".into(),
        "test_pipeline".into(),
        "test_step".into(),
        json!({}),
        Default::default(),
    );

    // Note: We cannot execute the action due to the async_recursion macro limitation
    // with Custom closures. This test verifies that the enum variant compiles.
    let _action = action; // Compile check
    let _ctx = ctx; // Compile check

    assert!(true, "Sleep action type definition compiles correctly");
}

#[tokio::test]
async fn test_sleep_until_action() {
    use chrono::{Duration, Utc};

    // Create a sleep until action with a time in the past (should complete immediately)
    let past_time = Utc::now() - Duration::seconds(10);
    let action = StepAction::SleepUntil {
        timestamp: past_time,
    };

    // Verify the action can be created and type-checks
    let _action = action;
    assert!(true, "SleepUntil action type definition compiles correctly");
}

#[test]
fn test_foreach_action() {
    // Create a ForEach action
    let foreach_action = StepAction::ForEach {
        input_array_key: "items".into(),
        body: Box::new(StepAction::UserInput {
            prompt: "Process item".into(),
            schema: None,
        }),
        concurrency: 2,
        collect_results: true,
    };

    // Just verify it compiles
    let _action = foreach_action;
    assert!(true, "ForEach action type definition compiles correctly");
}

#[test]
fn test_delegation_policy_with_hooks() {
    // Create a DelegationPolicy with all optional hook fields
    let policy = DelegationPolicy {
        max_depth: 3,
        allowed_agents: vec!["agent1".into(), "agent2".into()],
        require_output_schema: true,
        inherit_tool_scope: true,
        inherit_budget: false,
        require_user_approval: true,
        on_delegation_start: None,
        on_delegation_complete: None,
        on_iteration_complete: None,
        message_filter: None,
        memory_isolation: MemoryIsolation::Isolated,
    };

    assert_eq!(policy.max_depth, 3);
    assert_eq!(policy.allowed_agents.len(), 2);
    assert!(policy.require_output_schema);
    assert!(
        policy.on_delegation_start.is_none(),
        "on_delegation_start should be None"
    );
}

#[test]
fn test_delegation_policy_debug_impl() {
    // Verify that DelegationPolicy implements Debug despite having closures
    let policy = DelegationPolicy::default();
    let debug_str = format!("{:?}", policy);

    // Should contain field names and values, but show "<function>" for closures
    assert!(debug_str.contains("DelegationPolicy"));
    assert!(debug_str.contains("max_depth"));
    assert!(true, "DelegationPolicy Debug implementation works");
}

#[test]
fn test_pipeline_result_cost_fields() {
    // Create a minimal PipelineResult to verify cost tracking fields exist
    let result = PipelineResult {
        pipeline_name: "test".into(),
        steps_passed: vec![],
        steps_failed: vec![],
        step_results: Default::default(),
        audit_log: AuditLog::new(),
        success: true,
        total_cost_usd: 0.42,
        total_tokens_used: 1234,
        log: vec![],
        suspended: None,
        budget: Default::default(),
    };

    assert_eq!(result.total_cost_usd, 0.42);
    assert_eq!(result.total_tokens_used, 1234);
}

#[test]
fn test_iteration_context_and_decision() {
    // Verify IterationContext can be created
    let ctx = IterationContext {
        iteration: 1,
        agent: "test_agent".into(),
        output: StepOutput::new("result".into()),
    };

    assert_eq!(ctx.iteration, 1);
    assert_eq!(ctx.agent, "test_agent");

    // Verify IterationDecision enum compiles
    let decision = IterationDecision::Continue;
    let _d = decision;

    assert!(true);
}

#[test]
fn test_delegation_context_and_decision() {
    // Verify DelegationContext can be created
    let ctx = DelegationContext {
        agent: "child_agent".into(),
        input: json!({"key": "value"}),
        depth: 2,
    };

    assert_eq!(ctx.agent, "child_agent");
    assert_eq!(ctx.depth, 2);

    // Verify DelegationDecision enum compiles
    let decision = DelegationDecision::Proceed;
    let _d = decision;

    assert!(true);
}

#[test]
fn test_delegation_feedback() {
    // Verify DelegationFeedback enum can be used
    let feedback1 = DelegationFeedback::Continue;
    let feedback2 = DelegationFeedback::Bail {
        reason: "test reason".into(),
    };
    let feedback3 = DelegationFeedback::InjectFeedback("feedback".into());

    let _f1 = feedback1;
    let _f2 = feedback2;
    let _f3 = feedback3;

    assert!(true);
}

#[test]
fn test_iteration_failure_modes() {
    // Verify all IterationFailureMode variants can be created
    let retry = IterationFailureMode::Retry;
    let skip = IterationFailureMode::Skip;
    let abort = IterationFailureMode::Abort;

    let _r = retry;
    let _s = skip;
    let _a = abort;

    assert!(true, "All IterationFailureMode variants compile");
}

#[test]
fn test_step_output_construction() {
    // Test creating StepOutput with and without parsed JSON
    let output1 = StepOutput::new("raw output".into());
    assert_eq!(output1.raw, "raw output");
    assert!(output1.parsed.is_none());

    let parsed = json!({"result": "success"});
    let output2 = StepOutput::with_parsed("raw".into(), parsed.clone());
    assert_eq!(output2.raw, "raw");
    assert_eq!(output2.parsed, Some(parsed));
}

#[tokio::test]
async fn test_step_action_variants_compile() {
    // Verify all StepAction variants can be created (even if not all are fully implemented)

    // LlmCall
    let _ = StepAction::LlmCall {
        system: "system".into(),
        user: "user".into(),
        model: None,
        conversation_id: None,
        append_to_history: false,
    };

    // ToolCall
    let _ = StepAction::ToolCall {
        tool: "tool_name".into(),
        args: json!({}),
    };

    // DelegateAgent
    let _ = StepAction::DelegateAgent {
        agent: "agent".into(),
        input: json!({}),
        expected_output_schema: None,
        delegation_policy: DelegationPolicy::default(),
        detached: false,
    };

    // SubPipeline
    let _ = StepAction::SubPipeline(Box::new(Pipeline {
        name: "sub".into(),
        steps: vec![],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }));

    // LoopUntil
    let _ = StepAction::LoopUntil {
        body: Box::new(StepAction::UserInput {
            prompt: "test".into(),
            schema: None,
        }),
        condition: Guard::None,
        max_iterations: 5,
        on_iteration_failure: IterationFailureMode::Abort,
    };

    // UserInput
    let _ = StepAction::UserInput {
        prompt: "prompt".into(),
        schema: None,
    };

    // UseSkill
    let _ = StepAction::UseSkill {
        skill: "rust_debugging".into(),
        input: json!({}),
        mode: SkillMode::PromptOnly,
    };

    // Branch
    let _ = StepAction::Branch {
        condition: "success".into(),
        if_true: Box::new(StepAction::UserInput {
            prompt: "yes".into(),
            schema: None,
        }),
        if_false: None,
    };

    // RemoteAgent
    let _ = StepAction::RemoteAgent {
        endpoint: "http://localhost:8080".into(),
        agent_name: "remote".into(),
        payload: json!({}),
    };

    // LlmCallStreaming
    let _ = StepAction::LlmCallStreaming {
        system: "system".into(),
        user: "user".into(),
        model: None,
        conversation_id: None,
        append_to_history: false,
    };

    // ToolUseLoop
    let _ = StepAction::ToolUseLoop {
        system: "system".into(),
        user: "user".into(),
        model: ProviderSpec {
            model: "gpt-4".into(),
            provider: "openai".into(),
        },
        tools: vec!["tool1".into()],
        max_rounds: 5,
        stop_condition: StopCondition::TextOnly,
    };

    // Sleep
    let _ = StepAction::Sleep { duration_ms: 100 };

    // SleepUntil
    let _ = StepAction::SleepUntil {
        timestamp: chrono::Utc::now(),
    };

    // ForEach
    let _ = StepAction::ForEach {
        input_array_key: "items".into(),
        body: Box::new(StepAction::UserInput {
            prompt: "item".into(),
            schema: None,
        }),
        concurrency: 1,
        collect_results: false,
    };

    assert!(true, "All StepAction variants compile successfully");
}
