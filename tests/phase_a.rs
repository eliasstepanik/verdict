#![cfg(test)]

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

// A1 â€” Step-Level Tool Approval API
#[tokio::test]
async fn test_a1_tool_approval_registration() {
    let mut registry = ToolRegistry::with_builtins();
    let initial_count = registry.list().len();

    // Create a function tool using FunctionTool from prelude
    let my_tool = FunctionTool::new(
        "local.test_tool",
        "Test tool for approval",
        json!({"type": "object", "properties": {}}),
        |_args, _ctx| Box::pin(async { Ok(ToolOutput::text("test output".into())) }),
    );

    // Register with approval
    registry.register_with_approval(Arc::new(my_tool));

    // Verify tool is registered
    assert_eq!(registry.list().len(), initial_count + 1);

    // Verify it requires approval
    assert!(registry.requires_approval("local.test_tool"));
    assert!(!registry.requires_approval("fs.read"));
}

// A2 â€” Delegation Hooks (types exist, wiring deferred to runner)
#[test]
fn test_a2_delegation_hooks_types() {
    let ctx = DelegationContext {
        agent: "test".into(),
        input: json!({}),
        depth: 1,
    };
    assert_eq!(ctx.agent, "test");
    assert_eq!(ctx.depth, 1);

    let result = DelegationResult {
        agent: "test".into(),
        output: StepOutput::new("ok".into()),
        success: true,
    };
    assert!(result.success);

    // Verify enum variants exist
    let _decision = DelegationDecision::Proceed;
    let _feedback = DelegationFeedback::Continue;
    let _iteration_decision = IterationDecision::Continue;
}

// A3 â€” Sleep and SleepUntil
#[tokio::test]
async fn test_a3_sleep_step_action() {
    let action = StepAction::Sleep { duration_ms: 50 };

    match action {
        StepAction::Sleep { duration_ms } => {
            assert_eq!(duration_ms, 50);
        }
        _ => panic!("wrong variant"),
    }
}

#[tokio::test]
async fn test_a3_sleep_until_step_action() {
    let now = chrono::Utc::now();
    let future_time = now + chrono::Duration::seconds(5);

    let action = StepAction::SleepUntil {
        timestamp: future_time,
    };

    match action {
        StepAction::SleepUntil { timestamp } => {
            assert!(timestamp > now);
        }
        _ => panic!("wrong variant"),
    }
}

// A4 â€” ForEach Step Action
#[test]
fn test_a4_foreach_step_action() {
    let action = StepAction::ForEach {
        input_array_key: "items".into(),
        body: Box::new(StepAction::LlmCall {
            system: "Process one".into(),
            user: "item".into(),
            model: None,
            conversation_id: None,
            append_to_history: false,
        }),
        concurrency: 4,
        collect_results: true,
    };

    match action {
        StepAction::ForEach {
            input_array_key,
            concurrency,
            collect_results,
            ..
        } => {
            assert_eq!(input_array_key, "items");
            assert_eq!(concurrency, 4);
            assert!(collect_results);
        }
        _ => panic!("wrong variant"),
    }
}

// A5 â€” Guard::AllOfCollect
#[tokio::test]
async fn test_a5_guard_all_of_collect() {
    let guards = vec![Guard::NonEmptyOutput, Guard::ValidJson];

    let guard = Guard::AllOfCollect(guards);
    assert_eq!(guard.name(), "AllOfCollect");
}

// A6 â€” Cost Reporting in PipelineResult
#[test]
fn test_a6_pipeline_result_cost_fields() {
    let result = PipelineResult {
        pipeline_name: "test".into(),
        steps_passed: vec![],
        steps_failed: vec![],
        step_results: std::collections::HashMap::new(),
        audit_log: AuditLog::new(),
        success: true,
        total_cost_usd: 1.23,
        total_tokens_used: 456,
        log: vec![],
        suspended: None,
        budget: Default::default(),
    };

    assert_eq!(result.total_cost_usd, 1.23);
    assert_eq!(result.total_tokens_used, 456);
}

// A7 â€” Structured Logging
#[test]
fn test_a7_log_entry_and_level() {
    let entry = LogEntry {
        timestamp: chrono::Utc::now(),
        level: LogLevel::Info,
        pipeline: "test_pipeline".into(),
        step: "test_step".into(),
        trace_id: "trace-123".into(),
        span_id: "span-456".into(),
        message: "Step started".into(),
        fields: json!({"key": "value"}),
    };

    assert_eq!(entry.level, LogLevel::Info);
    assert_eq!(entry.pipeline, "test_pipeline");
}

#[test]
fn test_a7_output_event_log_variant() {
    let entry = LogEntry {
        timestamp: chrono::Utc::now(),
        level: LogLevel::Warn,
        pipeline: "test".into(),
        step: "step1".into(),
        trace_id: "t1".into(),
        span_id: "s1".into(),
        message: "warning".into(),
        fields: json!({}),
    };

    let event = OutputEvent::Log(entry);
    match event {
        OutputEvent::Log(e) => {
            assert_eq!(e.level, LogLevel::Warn);
        }
        _ => panic!("wrong variant"),
    }
}

// A8 â€” Thread Title Auto-Generation (types exist)
#[test]
fn test_a8_conversation_registry_title_support() {
    let mut registry = ConversationRegistry::new();

    // Verify methods exist
    registry.set_title("conv-1".into(), "Test Conversation".into());
    let title = registry.get_title("conv-1");
    assert_eq!(title, Some("Test Conversation"));
}

#[test]
fn test_a8_pipeline_runner_auto_title_method_exists() {
    // Verify PipelineRunner has with_auto_title_model builder method
    let provider = OpenAiCompatibleProvider::new(
        "https://api.openai.com".into(),
        "test-key".into(),
        "gpt-4".into(),
    );
    let client = Arc::new(LlmClient::new(Arc::new(provider)));
    let runner = PipelineRunner::new().with_auto_title_model(Arc::clone(&client));
    assert!(runner.auto_title_llm.is_some());
}

// Integration test for multiple Phase A features
#[tokio::test]
async fn test_phase_a_integration() {
    let mut registry = ToolRegistry::with_builtins();

    // A1: Set up tool approval
    let tool = FunctionTool::new(
        "local.sensitive_op",
        "Sensitive operation requiring approval",
        json!({"type": "object"}),
        |_args, _ctx| Box::pin(async { Ok(ToolOutput::text("done".into())) }),
    );
    registry.register_with_approval(Arc::new(tool));
    assert!(registry.requires_approval("local.sensitive_op"));

    // A5: Create AllOfCollect guard
    let guard = Guard::AllOfCollect(vec![Guard::NonEmptyOutput]);
    assert_eq!(guard.name(), "AllOfCollect");

    // A3: Create Sleep action
    let sleep_action = StepAction::Sleep { duration_ms: 100 };
    match sleep_action {
        StepAction::Sleep { duration_ms } => assert_eq!(duration_ms, 100),
        _ => panic!(),
    }

    // A4: Create ForEach action
    let foreach_action = StepAction::ForEach {
        input_array_key: "items".into(),
        body: Box::new(StepAction::LlmCall {
            system: "test".into(),
            user: "test".into(),
            model: None,
            conversation_id: None,
            append_to_history: false,
        }),
        concurrency: 2,
        collect_results: true,
    };
    match foreach_action {
        StepAction::ForEach { concurrency, .. } => assert_eq!(concurrency, 2),
        _ => panic!(),
    }

    // A6/A7: Verify PipelineResult has cost and log fields
    let result = PipelineResult {
        pipeline_name: "integration_test".into(),
        steps_passed: vec!["step1".into()],
        steps_failed: vec![],
        step_results: std::collections::HashMap::new(),
        audit_log: AuditLog::new(),
        success: true,
        total_cost_usd: 0.50,
        total_tokens_used: 250,
        log: vec![],
        suspended: None,
        budget: Default::default(),
    };

    assert!(result.success);
    assert_eq!(result.total_cost_usd, 0.50);
    assert!(result.log.is_empty());

    // A8: Verify ConversationRegistry title support
    let mut reg = ConversationRegistry::new();
    reg.set_title("test-conv".into(), "My Test Conversation".into());
    assert_eq!(reg.get_title("test-conv"), Some("My Test Conversation"));
}
