//! Probe: LoopUntil body coverage for tool denial.
//!
//! `handle_loop_until` (src/runner/step_executor.rs) re-dispatches to
//! `execute_action(body, ctx)` on the SAME `ctx` every iteration. It builds no
//! policy, no registry, no child context — so there is no second enforcement
//! path that could diverge from the parent's `ctx.allowed_tools`. Safe by
//! construction; these tests pin that property so a future refactor that
//! introduces a per-iteration context has to break a test to land.
//!
//! The body is a `SubPipeline` (a real delegation boundary) so the composition
//! LoopUntil -> SubPipeline -> ToolCall is covered, not just a bare ToolCall.
//! A counter tool proves the loop genuinely iterated N times, so "denied on
//! iteration 1 then never tried again" cannot masquerade as a pass.

mod common;

use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use verdict::prelude::*;

use common::delegation::{agent, pipeline, step, tripwire_tool};

/// Always-allowed tool that counts loop iterations.
fn counter_tool(count: Arc<AtomicUsize>) -> FunctionTool {
    FunctionTool::new(
        "test.counter_tool",
        "Increments a shared counter when executed.",
        json!({ "type": "object", "properties": {} }),
        move |_args, _ctx| {
            let count = count.clone();
            Box::pin(async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(ToolOutput::text("counted".into()))
            })
        },
    )
}

const LOOP_MAX_ITERATIONS: u32 = 3;

/// Builds: a single step whose action is
/// `LoopUntil { body: SubPipeline[count_tool, denied_tool] }`, retrying on
/// failure so the body runs on every one of `LOOP_MAX_ITERATIONS` passes.
///
/// The restriction is applied to the containing STEP's `tools` while the
/// agent's policy stays `ToolSet::Full`. This placement is deliberate and
/// load-bearing: `handle_subpipeline` clones `ctx.agent_policy`, so a denial
/// written into the AGENT policy would survive via that clone even with the
/// `narrow_child_tool_scope` call deleted — a test that cannot fail. Only a
/// step-level denial (which lives in `ctx.allowed_tools`, not
/// `ctx.agent_policy.allowed_tools`) actually exercises the narrowing.
///
/// Returns (denied tool executed?, iteration count, pipeline outcome).
async fn run_loop(
    step_tools: ToolSet,
) -> (bool, usize, Result<PipelineResult, PipelineError>) {
    let flag = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));

    let mut tools = ToolRegistry::with_builtins();
    tools.register(tripwire_tool(flag.clone()));
    tools.register(counter_tool(count.clone()));

    // Inner pipeline's own steps are wide open (`ToolSet::Full`); only an
    // inherited restriction from the containing step can stop the tool.
    let body = StepAction::SubPipeline(Box::new(pipeline(
        "loop_body",
        vec![
            step(
                "count_iteration",
                StepAction::ToolCall {
                    tool: "test.counter_tool".into(),
                    args: json!({}),
                },
                ToolSet::Full,
            ),
            step(
                "call_denied_tool",
                StepAction::ToolCall {
                    tool: "test.denied_tool".into(),
                    args: json!({}),
                },
                ToolSet::Full,
            ),
        ],
    )));

    let loop_pipeline = pipeline(
        "loop_pipeline",
        vec![step(
            "loop_step",
            StepAction::LoopUntil {
                body: Box::new(body),
                condition: Guard::None,
                max_iterations: LOOP_MAX_ITERATIONS,
                on_iteration_failure: IterationFailureMode::Retry,
            },
            step_tools,
        )],
    );
    let looper = agent("looper", loop_pipeline.clone(), ToolSet::Full);

    let result = PipelineRunner::with_registries(Arc::new(tools), Arc::new(AgentRegistry::new()))
        .run(&loop_pipeline, &looper, json!({}))
        .await;

    (flag.load(Ordering::SeqCst), count.load(Ordering::SeqCst), result)
}

/// A tool denied at the containing step's level must stay denied inside a
/// `LoopUntil` body — on every iteration, not just the first.
#[tokio::test]
async fn loop_body_tool_stays_denied_across_iterations() {
    let (tool_ran, iterations, result) =
        run_loop(ToolSet::Deny(vec!["test.denied_tool".into()])).await;

    assert!(
        !tool_ran,
        "ESCALATION: 'test.denied_tool' is denied by the containing step's tool \
         scope, yet it executed inside a LoopUntil body. Pipeline result: {result:?}"
    );
    assert_eq!(
        iterations, LOOP_MAX_ITERATIONS as usize,
        "harness broken: the loop body should have run {LOOP_MAX_ITERATIONS} times \
         (body fails on the denied tool, IterationFailureMode::Retry), but ran \
         {iterations}. The denial test only proves per-iteration enforcement if \
         multiple iterations actually happened. Pipeline result: {result:?}"
    );
}

/// Positive control: without a denial the same loop MUST reach and run the
/// tool, exiting after one iteration once `Guard::None` passes.
#[tokio::test]
async fn positive_control_loop_body_tool_runs_without_deny() {
    let (tool_ran, iterations, result) = run_loop(ToolSet::Full).await;

    assert!(
        tool_ran,
        "harness broken: unrestricted loop never reached the tool call, so the \
         denial test proves nothing. Pipeline result: {result:?}"
    );
    assert_eq!(
        iterations, 1,
        "harness broken: body succeeds and Guard::None passes, so the loop should \
         exit after one iteration, got {iterations}. Pipeline result: {result:?}"
    );
}
