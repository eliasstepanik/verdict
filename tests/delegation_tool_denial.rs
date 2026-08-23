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

use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use verdict::prelude::*;

/// A tool that records the fact it was actually executed.
fn tripwire_tool(flag: Arc<AtomicBool>) -> FunctionTool {
    FunctionTool::new(
        "test.denied_tool",
        "Flips a shared flag when executed.",
        json!({ "type": "object", "properties": {} }),
        move |_args, _ctx| {
            let flag = flag.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
                Ok(ToolOutput::text("tool ran".into()))
            })
        },
    )
}

fn step(name: &str, action: StepAction, tools: ToolSet) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action,
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

fn pipeline(name: &str, steps: Vec<AgentStep>) -> Pipeline {
    Pipeline {
        name: name.into(),
        steps,
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

fn agent(name: &str, p: Pipeline, allowed_tools: ToolSet) -> Agent {
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = allowed_tools.clone();
    policy.allowed_agents = vec!["level2".into()];
    Agent {
        name: name.into(),
        description: name.into(),
        pipeline: p,
        tools: allowed_tools,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    }
}

/// Builds: level1 (parent, scope = `parent_tools`) --DelegateAgent--> level2,
/// whose own policy is wide open and whose step calls `test.denied_tool`.
/// Returns whether the tool actually executed, plus the pipeline outcome.
async fn run_chain(parent_tools: ToolSet) -> (bool, Result<PipelineResult, PipelineError>) {
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
                detached: false,
            },
            ToolSet::Full,
        )],
    );
    let level1 = agent("level1", level1_pipeline.clone(), parent_tools);

    let result = PipelineRunner::with_registries(Arc::new(tools), Arc::new(agents))
        .run(&level1_pipeline, &level1, json!({}))
        .await;

    (flag.load(Ordering::SeqCst), result)
}

/// THE PROBE. A tool denied by the parent agent's policy must not execute in a
/// delegated child, no matter how permissive that child's own policy is.
#[tokio::test]
async fn parent_deny_binds_delegated_child() {
    let (tool_ran, result) = run_chain(ToolSet::Deny(vec!["test.denied_tool".into()])).await;

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
    let (tool_ran, result) = run_chain(ToolSet::Full).await;

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
