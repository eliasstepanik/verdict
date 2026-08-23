//! Probe: does a tool DENIED at the STEP level stay denied inside that step's
//! `FailureMode::Fallback` pipeline?
//!
//! `handle_fallback` (`src/runner/fallback.rs`) rebuilds the fallback agent's
//! policy from `ctx.agent_policy` — the AGENT-level default. It previously
//! stopped there, arguing each fallback step re-intersects with its own declared
//! `tools`. That argument only covers *further narrowing from the agent default*;
//! it cannot reconstruct a restriction the FAILED STEP imposed. So a step scoped
//! `ToolSet::Deny(["test.denied_tool"])` whose fallback declared `ToolSet::Full`
//! executed the denied tool — the same widening-vs-narrowing conflation behind
//! the `DelegateAgent` and `SubPipeline` escalations.
//!
//! The observation is a real side effect: the tool flips an `AtomicBool`. A
//! string match on an error message would not prove the tool never ran.
//!
//! Three flags guard against a vacuous pass:
//!   * `denied` — the escalation tripwire, must never flip under deny.
//!   * `marker` — a DIFFERENT tool called first inside the fallback, proving the
//!     fallback pipeline genuinely executed even when the escalation is blocked.
//!   * the positive control below, proving the tool is reachable without deny.

mod common;

use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use verdict::prelude::*;

use common::delegation::{agent, step, tripwire_tool};

/// A second tripwire under a name no test denies. Flipping proves the fallback
/// pipeline actually ran, so a non-flip of the denied tool means "blocked",
/// not "never reached".
fn marker_tool(flag: Arc<AtomicBool>) -> FunctionTool {
    FunctionTool::new(
        "test.marker_tool",
        "Flips a shared flag when the fallback pipeline executes.",
        json!({ "type": "object", "properties": {} }),
        move |_args, _ctx| {
            let flag = flag.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
                Ok(ToolOutput::text("fallback ran".into()))
            })
        },
    )
}

fn pipeline_with_fallback(name: &str, steps: Vec<AgentStep>, fallback: Pipeline) -> Pipeline {
    Pipeline {
        name: name.into(),
        steps,
        on_failure: FailureMode::Fallback(Box::new(fallback)),
        max_retries: 0,
    }
}

/// Builds: agent scoped `ToolSet::Full`, one step scoped `step_tools` that FAILS
/// (it calls an unregistered tool), whose fallback pipeline is scoped
/// `ToolSet::Full` and calls `test.marker_tool` then `test.denied_tool`.
///
/// Returns `(denied_tool_ran, fallback_ran, outcome)`.
async fn run_fallback(step_tools: ToolSet) -> (bool, bool, Result<PipelineResult, PipelineError>) {
    let denied = Arc::new(AtomicBool::new(false));
    let marker = Arc::new(AtomicBool::new(false));

    let mut tools = ToolRegistry::with_builtins();
    tools.register(tripwire_tool(denied.clone()));
    tools.register(marker_tool(marker.clone()));

    // The fallback's own scope is wide open. Only a restriction inherited from
    // the failed step can stop it.
    let fallback = Pipeline {
        name: "fallback_pipeline".into(),
        steps: vec![
            step(
                "fallback_marker",
                StepAction::ToolCall {
                    tool: "test.marker_tool".into(),
                    args: json!({}),
                },
                ToolSet::Full,
            ),
            step(
                "fallback_escalate",
                StepAction::ToolCall {
                    tool: "test.denied_tool".into(),
                    args: json!({}),
                },
                ToolSet::Full,
            ),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // The primary step fails: `test.no_such_tool` is not registered.
    let main = pipeline_with_fallback(
        "main_pipeline",
        vec![step(
            "failing_step",
            StepAction::ToolCall {
                tool: "test.no_such_tool".into(),
                args: json!({}),
            },
            step_tools,
        )],
        fallback,
    );

    // Agent policy is deliberately `ToolSet::Full`: the ONLY restriction in play
    // is the failing step's own scope, which is exactly what the fallback used
    // to discard.
    let a = agent("main", main.clone(), ToolSet::Full);

    let result = PipelineRunner::with_registries(Arc::new(tools), Arc::new(AgentRegistry::new()))
        .run(&main, &a, json!({}))
        .await;

    (
        denied.load(Ordering::SeqCst),
        marker.load(Ordering::SeqCst),
        result,
    )
}

/// THE PROBE. A tool denied by the FAILING STEP's scope must not execute inside
/// that step's fallback pipeline, however permissive the fallback declares itself.
#[tokio::test]
async fn step_deny_binds_fallback_pipeline() {
    let (denied_ran, fallback_ran, result) =
        run_fallback(ToolSet::Deny(vec!["test.denied_tool".into()])).await;

    assert!(
        fallback_ran,
        "harness broken: the fallback pipeline never ran, so the denial assertion \
         below would be vacuous. Pipeline result: {result:?}"
    );
    assert!(
        !denied_ran,
        "ESCALATION: 'test.denied_tool' is denied by the failing step's scope, yet \
         it executed inside that step's fallback pipeline. Pipeline result: {result:?}"
    );
}

/// Positive control: with no denial the exact same structure MUST reach and run
/// the tool, so a non-flip above means "blocked", not "unreachable".
#[tokio::test]
async fn positive_control_fallback_tool_runs_without_deny() {
    let (denied_ran, fallback_ran, result) = run_fallback(ToolSet::Full).await;

    assert!(
        fallback_ran,
        "harness broken: unrestricted fallback never ran, got {result:?}"
    );
    assert!(
        denied_ran,
        "harness broken: unrestricted fallback never reached the tool call, so the \
         denial test proves nothing. Pipeline result: {result:?}"
    );
    assert!(
        result.as_ref().map(|r| r.success).unwrap_or(false),
        "harness broken: unrestricted fallback pipeline should succeed, got {result:?}"
    );
}
