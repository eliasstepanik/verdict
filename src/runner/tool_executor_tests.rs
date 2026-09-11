use super::extract_shell_command_string;
use super::PipelineRunner;
use crate::action::StepError;
use crate::agent::FilesystemPolicy;
use crate::audit::AuditEvent;
use crate::context::StepContext;
use crate::registry::ToolRegistry;
use crate::runner::{OutputEvent, OutputSink};
use crate::tools::{FunctionTool, ToolOutput};
use crate::toolset::ToolSet;
use std::sync::{Arc, Mutex};

#[test]
fn test_extract_shell_run_command() {
    let args = serde_json::json!({
        "command": "rm",
        "args": ["-rf", "/tmp"]
    });
    let result = extract_shell_command_string("shell_run", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "rm -rf /tmp");
}

#[test]
fn test_extract_shell_run_command_tool_run_command_variant() {
    // Critical test: shell_run_command must extract the command the same way as shell_run
    let args = serde_json::json!({
        "command": "rm",
        "args": ["-rf", "/tmp"]
    });
    let result = extract_shell_command_string("shell_run_command", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "rm -rf /tmp");
}

#[test]
fn test_extract_shell_cargo_test() {
    let args = serde_json::json!({});
    let result = extract_shell_command_string("shell_cargo_test", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cargo test");
}

#[test]
fn test_extract_shell_unknown_fallback() {
    let args = serde_json::json!({});
    let result = extract_shell_command_string("shell_custom_tool", &args);
    assert!(result.is_ok());
    // Should strip the "shell_" prefix
    assert_eq!(result.unwrap(), "custom_tool");
}

#[test]
fn test_extract_shell_run_command_with_no_args() {
    let args = serde_json::json!({
        "command": "cargo"
    });
    let result = extract_shell_command_string("shell_run_command", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cargo");
}

// ─── ADR-088: fail-closed tool-approval enforcement ────────────────────────

/// A sink that records every emitted `OutputEvent::ToolApprovalRequired`.
struct RecordingSink {
    approval_events: Arc<Mutex<Vec<(String, String)>>>, // (step, tool)
}

#[async_trait::async_trait]
impl OutputSink for RecordingSink {
    async fn emit(&self, event: OutputEvent) {
        if let OutputEvent::ToolApprovalRequired { step, tool, .. } = event {
            self.approval_events.lock().unwrap().push((step, tool));
        }
    }
}

fn approval_test_tool(name: &str) -> FunctionTool {
    FunctionTool::new(
        name.to_string(),
        "test tool".to_string(),
        serde_json::json!({"type": "object", "properties": {}}),
        |_args, _ctx| Box::pin(async { Ok(ToolOutput::text("executed".to_string())) }),
    )
}

fn test_ctx() -> StepContext {
    let mut ctx = StepContext::new(
        "agent".to_string(),
        "pipeline".to_string(),
        "step".to_string(),
        serde_json::json!({}),
        FilesystemPolicy::default(),
    );
    ctx.allowed_tools = ToolSet::Full;
    ctx
}

/// (a) FAIL-CLOSED PROOF: approval-required tool, no `approval_decision`
/// configured (the default) => the call is genuinely DENIED, not silently
/// allowed.
#[tokio::test]
async fn test_approval_required_tool_denied_by_default() {
    let mut registry = ToolRegistry::new();
    registry.register_with_approval(Arc::new(approval_test_tool("gated_tool")));

    let runner = PipelineRunner::with_tool_registry(Arc::new(registry));
    assert!(runner.approval_decision.is_none(), "default must be None");

    let mut ctx = test_ctx();
    let result = runner
        .execute_tool_call("gated_tool", &serde_json::json!({}), &mut ctx)
        .await;

    match result {
        Err(StepError::ActionFailed { reason }) => {
            assert!(
                reason.contains("requires approval") && reason.contains("denied"),
                "unexpected denial reason: {reason}"
            );
        }
        other => panic!("expected fail-closed denial, got {other:?}"),
    }
}

/// (b) approval_decision returning `true` => the tool is genuinely ALLOWED
/// (it actually executes and returns its output).
#[tokio::test]
async fn test_approval_required_tool_allowed_when_approved() {
    let mut registry = ToolRegistry::new();
    registry.register_with_approval(Arc::new(approval_test_tool("gated_tool")));

    let runner = PipelineRunner::with_tool_registry(Arc::new(registry))
        .with_approval_decision(Arc::new(|_name, _args| true));

    let mut ctx = test_ctx();
    let result = runner
        .execute_tool_call("gated_tool", &serde_json::json!({}), &mut ctx)
        .await;

    match result {
        Ok(output) => assert_eq!(output.raw, "executed"),
        Err(e) => panic!("expected the tool to execute, got error: {e}"),
    }
}

/// (c) approval_decision returning `false` => genuinely DENIED (explicit
/// opt-out, same outcome as the default but reached via an explicit `false`).
#[tokio::test]
async fn test_approval_required_tool_denied_when_explicitly_rejected() {
    let mut registry = ToolRegistry::new();
    registry.register_with_approval(Arc::new(approval_test_tool("gated_tool")));

    let runner = PipelineRunner::with_tool_registry(Arc::new(registry))
        .with_approval_decision(Arc::new(|_name, _args| false));

    let mut ctx = test_ctx();
    let result = runner
        .execute_tool_call("gated_tool", &serde_json::json!({}), &mut ctx)
        .await;

    match result {
        Err(StepError::ActionFailed { reason }) => {
            assert!(reason.contains("requires approval"), "reason: {reason}");
        }
        other => panic!("expected explicit denial, got {other:?}"),
    }
}

/// (d) purely additive: a NON-approval-required tool is completely
/// unaffected by `approval_decision`'s presence/absence — runs normally in
/// both cases, proving zero regression to existing tool-call behavior.
#[tokio::test]
async fn test_non_approval_tool_unaffected_by_approval_decision() {
    // No approval_decision configured at all.
    let mut registry_a = ToolRegistry::new();
    registry_a.register(approval_test_tool("plain_tool"));
    let runner_a = PipelineRunner::with_tool_registry(Arc::new(registry_a));
    let mut ctx_a = test_ctx();
    let result_a = runner_a
        .execute_tool_call("plain_tool", &serde_json::json!({}), &mut ctx_a)
        .await;
    assert_eq!(result_a.unwrap().raw, "executed");

    // approval_decision configured but returning false — must still be
    // irrelevant since this tool never required approval.
    let mut registry_b = ToolRegistry::new();
    registry_b.register(approval_test_tool("plain_tool"));
    let runner_b = PipelineRunner::with_tool_registry(Arc::new(registry_b))
        .with_approval_decision(Arc::new(|_name, _args| false));
    let mut ctx_b = test_ctx();
    let result_b = runner_b
        .execute_tool_call("plain_tool", &serde_json::json!({}), &mut ctx_b)
        .await;
    assert_eq!(result_b.unwrap().raw, "executed");
}

/// (e) the denial is genuinely audit-logged: `execute_tool_call` builds its
/// own local audit log (mirrored back onto `self.audit_log` by callers like
/// `handle_tool_call`), so assert directly on the log it appends to here —
/// this is the same pattern the existing ToolCallFailed assertions in
/// `handle_tool_call`'s Err branch rely on.
#[tokio::test]
async fn test_denial_is_audit_logged() {
    let mut registry = ToolRegistry::new();
    registry.register_with_approval(Arc::new(approval_test_tool("gated_tool")));

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(registry));
    let mut ctx = test_ctx();

    // Route through handle_tool_call (not execute_tool_call directly) since
    // that's the path that writes onto `self.audit_log`, matching how a real
    // ToolCall step observes PipelineResult.audit_log.
    let result = runner
        .handle_tool_call(&mut ctx, "gated_tool", &serde_json::json!({}))
        .await;
    assert!(result.is_err(), "expected denial");

    let logged_failure = runner.audit_log.entries().into_iter().any(|e| {
        matches!(
            &e.event,
            AuditEvent::ToolCallFailed { tool, reason }
                if tool == "gated_tool" && reason.contains("requires approval")
        )
    });
    assert!(
        logged_failure,
        "expected a ToolCallFailed audit entry recording the approval denial"
    );
}

/// (f) `OutputEvent::ToolApprovalRequired` is genuinely emitted for an
/// approval-required tool call, regardless of whether it's ultimately
/// approved or denied.
#[tokio::test]
async fn test_approval_required_event_emitted_regardless_of_outcome() {
    type Decision = Option<crate::tools::ApprovalDecision>;
    let cases: Vec<(&str, Decision)> = vec![
        ("denied-by-default", None),
        (
            "explicitly-approved",
            Some(Arc::new(|_n: &str, _a: &serde_json::Value| true)),
        ),
        (
            "explicitly-denied",
            Some(Arc::new(|_n: &str, _a: &serde_json::Value| false)),
        ),
    ];

    for (label, decision) in cases {
        let mut registry = ToolRegistry::new();
        registry.register_with_approval(Arc::new(approval_test_tool("gated_tool")));

        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::new(RecordingSink {
            approval_events: events.clone(),
        });

        let mut runner = PipelineRunner::with_tool_registry(Arc::new(registry))
            .with_output_sink(sink);
        if let Some(f) = decision {
            runner = runner.with_approval_decision(f);
        }

        let mut ctx = test_ctx();
        let _ = runner
            .execute_tool_call("gated_tool", &serde_json::json!({}), &mut ctx)
            .await;

        let recorded = events.lock().unwrap().clone();
        assert_eq!(
            recorded,
            vec![("step".to_string(), "gated_tool".to_string())],
            "ToolApprovalRequired must be emitted exactly once for case '{label}'"
        );
    }
}
