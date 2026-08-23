//! Probe: does a tool DENIED at a parent agent's policy stay denied inside an
//! agent reached via `StepAction::DelegateAgent`?
//!
//! `SubPipeline` narrows the child's scope from the parent
//! (`step_executor.rs`: `policy.allowed_tools = ctx.allowed_tools.clone()`).
//! `delegation.rs` has no equivalent — `inherit_tool_scope` only picks WHICH
//! `ToolRegistry` the child gets (permission-widening), and the child's
//! effective scope is recomputed from the CHILD's own `policy.allowed_tools`.
//!
//! The observation is a real side effect: the tool flips an `AtomicBool`. A
//! string match on an error message would not prove the tool never ran.
//!
//! Both values of `StepAction::DelegateAgent::detached` are covered. `detached`
//! is a declarative marker only: both dispatch sites (`step_exec::run_action`
//! and `execution.rs`'s retry loop) destructure it as `detached: _` and route
//! every delegation through the same `execute_delegation`, which is where
//! `narrow_child_tool_scope` clamps the child. There is no fire-and-forget
//! spawn (`src/runner/` contains no `tokio::spawn`), so no second policy
//! construction path exists that could bypass the clamp. These tests pin that
//! property down so a future async detached implementation cannot silently
//! reintroduce the escalation.

mod common;

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

use common::delegation::{agent, pipeline, step, tripwire_tool};
use std::sync::atomic::AtomicBool;

/// Builds: level1 (parent, scope = `parent_tools`) --DelegateAgent--> level2,
/// whose own policy is wide open and whose step calls `test.denied_tool`.
/// `detached` sets the corresponding `DelegateAgent` field.
/// Returns whether the tool actually executed, plus the pipeline outcome.
async fn run_chain(
    parent_tools: ToolSet,
    detached: bool,
) -> (bool, Result<PipelineResult, PipelineError>) {
    let flag = Arc::new(AtomicBool::new(false));

    let mut tools = ToolRegistry::with_builtins();
    tools.register(tripwire_tool(flag.clone()));

    // Grandchild: its OWN policy permits everything. Only an inherited
    // restriction from level1 can stop it.
    let level2 = agent(
        "level2",
        pipeline(
            "level2_pipeline",
            vec![step(
                "call_denied_tool",
                StepAction::ToolCall {
                    tool: "test.denied_tool".into(),
                    args: json!({}),
                },
                ToolSet::Full,
            )],
        ),
        ToolSet::Full,
    );

    let mut agents = AgentRegistry::new();
    agents.register(level2);

    let level1_pipeline = pipeline(
        "level1_pipeline",
        vec![step(
            "delegate",
            StepAction::DelegateAgent {
                agent: "level2".into(),
                input: json!({}),
                expected_output_schema: None,
                delegation_policy: DelegationPolicy::default(),
                detached,
            },
            ToolSet::Full,
        )],
    );
    let level1 = agent("level1", level1_pipeline.clone(), parent_tools);

    let result = PipelineRunner::with_registries(Arc::new(tools), Arc::new(agents))
        .run(&level1_pipeline, &level1, json!({}))
        .await;

    (flag.load(std::sync::atomic::Ordering::SeqCst), result)
}

/// THE PROBE. A tool denied by the parent agent's policy must not execute in a
/// delegated child, no matter how permissive that child's own policy is.
#[tokio::test]
async fn parent_deny_binds_delegated_child() {
    let (tool_ran, result) = run_chain(ToolSet::Deny(vec!["test.denied_tool".into()]), false).await;

    assert!(
        !tool_ran,
        "ESCALATION: 'test.denied_tool' is denied by level1's policy, yet it \
         executed inside delegated agent 'level2'. Pipeline result: {result:?}"
    );
}

/// Positive control: with no denial in place the exact same chain MUST reach
/// and run the tool. Without this, the test above could "pass" simply because
/// the pipeline never got as far as the tool call.
#[tokio::test]
async fn positive_control_tool_runs_without_deny() {
    let (tool_ran, result) = run_chain(ToolSet::Full, false).await;

    assert!(
        result.as_ref().map(|r| r.success).unwrap_or(false),
        "harness broken: unrestricted delegation chain should succeed, got {result:?}"
    );
    assert!(
        tool_ran,
        "harness broken: unrestricted chain never reached the tool call, so the \
         denial test proves nothing. Pipeline result: {result:?}"
    );
}

/// Same probe with `detached: true`. Detached delegation must be bound by the
/// parent's denial exactly like the synchronous path — it shares
/// `execute_delegation`, and therefore `narrow_child_tool_scope`, rather than
/// constructing the child's policy independently.
#[tokio::test]
async fn parent_deny_binds_detached_delegated_child() {
    let (tool_ran, result) = run_chain(ToolSet::Deny(vec!["test.denied_tool".into()]), true).await;

    assert!(
        !tool_ran,
        "ESCALATION: 'test.denied_tool' is denied by level1's policy, yet it \
         executed inside DETACHED delegated agent 'level2'. Pipeline result: {result:?}"
    );
}

/// Positive control for the detached path. Proves the detached chain actually
/// reaches the tool call, so the detached denial test above is not vacuous.
#[tokio::test]
async fn positive_control_detached_tool_runs_without_deny() {
    let (tool_ran, result) = run_chain(ToolSet::Full, true).await;

    assert!(
        result.as_ref().map(|r| r.success).unwrap_or(false),
        "harness broken: unrestricted DETACHED delegation chain should succeed, got {result:?}"
    );
    assert!(
        tool_ran,
        "harness broken: unrestricted detached chain never reached the tool call, so \
         the detached denial test proves nothing. Pipeline result: {result:?}"
    );
}
