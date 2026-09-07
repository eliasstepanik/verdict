//! Integration tests for VERDICT-CHANGE-1: the pre-execution `ToolGuard` hook
//! on `PipelineRunner`.
//!
//! Mirrors the real-pipeline probe style of `probe_shell_denylist_real.rs`:
//! builds a genuine `Pipeline` + `Agent`, runs it through the real
//! `PipelineRunner` with the real builtin `ToolRegistry`, and proves the
//! guard rejects the call BEFORE the tool's side effect (file creation via
//! `touch`) ever occurs.

use serde_json::json;
use verdict::prelude::*;

/// Build a one-step pipeline that calls a real shell tool.
fn shell_step(tool: &str, command: &str, args: Vec<&str>) -> AgentStep {
    AgentStep {
        name: "shell_step".into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall {
            tool: tool.into(),
            args: json!({
                "command": command,
                "args": args,
            }),
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

fn probe_pipeline_and_agent(step: AgentStep) -> (Pipeline, Agent) {
    let pipeline = Pipeline {
        name: "tool_guard_probe".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Full;
    let agent = Agent {
        name: "probe_agent".into(),
        description: "tool guard probe".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    };
    (pipeline, agent)
}

/// A test guard: rejects `shell_run` outright, allows everything else.
fn deny_shell_run_guard() -> ToolGuard {
    Box::new(|tool_name, _args, _ctx| {
        if tool_name == "shell_run" {
            Err("blocked by test guard".to_string())
        } else {
            Ok(())
        }
    })
}

/// The guard must reject the call BEFORE the tool runs: no canary file is
/// ever created, and the error surfaces the guard's exact rejection message.
#[tokio::test]
async fn test_tool_guard_rejects_call_before_execution() {
    let canary_rel = format!("tool_guard_canary_{}.txt", std::process::id());
    let canary_abs = std::env::current_dir().unwrap().join(&canary_rel);
    let _ = std::fs::remove_file(&canary_abs);

    let step = shell_step("shell_run", "touch", vec![canary_rel.as_str()]);
    let (pipeline, agent) = probe_pipeline_and_agent(step);

    let mut runner = PipelineRunner::new().with_tool_guards(vec![deny_shell_run_guard()]);
    let result = runner.run(&pipeline, &agent, json!({})).await;

    let canary_created = canary_abs.exists();
    let _ = std::fs::remove_file(&canary_abs);

    assert!(
        !canary_created,
        "CRITICAL: guard rejected the call but the tool still ran (canary file was created)"
    );

    match result {
        Err(PipelineError::StepFailed { error, .. }) => {
            let msg = format!("{:?}", error);
            assert!(
                msg.contains("blocked by test guard"),
                "expected the guard's exact rejection message in the error, got: {}",
                msg
            );
            assert!(
                msg.contains("rejected by guard"),
                "expected the 'rejected by guard' wrapper text, got: {}",
                msg
            );
        }
        other => panic!(
            "expected StepFailed carrying the guard rejection, got: {:?}",
            other
        ),
    }
}

/// Default behavior (`tool_guards: None`) must be unchanged: an unguarded
/// pipeline can call tools normally — no regression.
#[tokio::test]
async fn test_no_guard_configured_behaves_as_before() {
    let canary_rel = format!("tool_guard_canary_none_{}.txt", std::process::id());
    let canary_abs = std::env::current_dir().unwrap().join(&canary_rel);
    let _ = std::fs::remove_file(&canary_abs);

    let step = shell_step("shell_run", "touch", vec![canary_rel.as_str()]);
    let (pipeline, agent) = probe_pipeline_and_agent(step);

    // No .with_tool_guards(..) call at all — tool_guards stays None.
    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, json!({})).await;

    let canary_created = canary_abs.exists();
    let _ = std::fs::remove_file(&canary_abs);

    assert!(
        result.is_ok(),
        "unguarded pipeline should succeed normally, got: {:?}",
        result
    );
    assert!(
        canary_created,
        "unguarded shell_run should have actually executed and created the canary file"
    );
}
