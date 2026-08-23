//! Regression (C4): `FailureMode::Fallback` runs a substitute pipeline for the
//! step that just failed, so it must inherit the parent's security context.
//!
//! The handler used to build a fresh `PipelineRunner` and call the plain `run()`
//! entry point, which reset `delegation_depth` to 0 and restarted the budget. A
//! delegated child sitting at `max_delegation_depth` could therefore launder
//! unlimited further delegation through its fallback pipeline.
//!
//! Depth semantics under test: a fallback replaces the failed step *in place*, so
//! it runs at the SAME depth as that step — not reset to 0, and not incremented
//! the way a `SubPipeline` boundary is.
//!
//! Every assertion here observes a real effect (the depth a fallback step actually
//! sees, real budget counters, a real blocked run), never internal state.

use serde_json::json;
use std::sync::{Arc, Mutex};
use verdict::prelude::*;

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

fn failing_step(name: &str) -> AgentStep {
    failing_step_scoped(name, ToolSet::None)
}

/// A failing step carrying an explicit tool scope.
///
/// The fallback inherits the FAILED STEP's effective tool scope (a fallback is
/// the same logical step retried a different way, so it inherits that step's
/// ceiling alongside its depth and budget — `src/runner/fallback.rs`). A step
/// scoped `ToolSet::None` therefore yields a fallback that can call nothing,
/// which is correct: "None because this step needs no tools" and "None because
/// this step is restricted" are indistinguishable, and resolving that ambiguity
/// in favour of the permissive reading is exactly the widening-vs-narrowing
/// conflation that caused the escalation. Tests whose fallback must actually
/// call a tool therefore scope the failing step to permit that tool.
fn failing_step_scoped(name: &str, tools: ToolSet) -> AgentStep {
    step(
        name,
        StepAction::Custom(Arc::new(|_ctx| {
            Err(StepError::ActionFailed {
                reason: "forced failure to trigger fallback".into(),
            })
        })),
        tools,
    )
}

fn pipeline(name: &str, steps: Vec<AgentStep>, on_failure: FailureMode) -> Pipeline {
    Pipeline {
        name: name.into(),
        steps,
        on_failure,
        max_retries: 0,
    }
}

fn agent(p: &Pipeline, policy: AgentPolicy) -> Agent {
    Agent {
        name: "parent".into(),
        description: "parent".into(),
        pipeline: p.clone(),
        tools: policy.allowed_tools.clone(),
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    }
}

/// C4.1: the fallback must observe the parent's delegation depth (3), not a reset
/// depth (0) and not an incremented one (4).
#[tokio::test]
async fn fallback_observes_parent_delegation_depth() {
    let observed: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let sink = observed.clone();

    let fallback = pipeline(
        "fallback",
        vec![step(
            "observe",
            StepAction::Custom(Arc::new(move |ctx| {
                *sink.lock().unwrap() = Some(ctx.delegation_depth);
                Ok(StepOutput::new("ok".into()))
            })),
            ToolSet::None,
        )],
        FailureMode::Abort,
    );

    let main = pipeline(
        "main",
        vec![failing_step("boom")],
        FailureMode::Fallback(Box::new(fallback)),
    );

    let mut policy = AgentPolicy::default();
    // Headroom, so the run is not incidentally blocked by the depth cap.
    policy.max_delegation_depth = 10;
    let a = agent(&main, policy);

    // Enter the parent at depth 3, as a delegated agent would be.
    PipelineRunner::new()
        .run_with_delegation_depth(&main, &a, json!({}), 3, "grandparent".into())
        .await
        .expect("fallback should rescue the pipeline");

    let depth = observed
        .lock()
        .unwrap()
        .expect("fallback step must have run");
    assert_eq!(
        depth, 3,
        "fallback runs at the SAME depth as the failed step (3); \
         0 means the depth was reset, 4 means it was wrongly treated as a nesting boundary"
    );
}

/// C4.2: budget must be continuous — the fallback sees the parent's prior spend,
/// and spend inside the fallback accumulates into the run's final budget.
#[tokio::test]
async fn fallback_inherits_and_accumulates_budget() {
    let observed: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let sink = observed.clone();

    // The fallback observes the inherited spend, then spends again itself.
    let fallback = pipeline(
        "fallback",
        vec![
            step(
                "observe",
                StepAction::Custom(Arc::new(move |ctx| {
                    *sink.lock().unwrap() = Some(ctx.budget.tool_calls_used);
                    Ok(StepOutput::new("ok".into()))
                })),
                ToolSet::None,
            ),
            step(
                "spend_again",
                StepAction::ToolCall {
                    tool: "fs.list".into(),
                    args: json!({ "path": "." }),
                },
                ToolSet::Allow(vec!["fs.list".into()]),
            ),
        ],
        FailureMode::Abort,
    );

    // A real tool call in the parent increments `tool_calls_used` before the
    // failing step hands off to the fallback.
    let bump = step(
        "bump",
        StepAction::ToolCall {
            tool: "fs.list".into(),
            args: json!({ "path": "." }),
        },
        ToolSet::Allow(vec!["fs.list".into()]),
    );

    // The failing step is scoped to permit `fs.list` because its fallback calls
    // `fs.list`; the fallback inherits this step's scope. This test asserts budget
    // continuity, not tool scoping (that is `delegation_tool_denial_fallback.rs`).
    let main = pipeline(
        "main",
        vec![
            bump,
            failing_step_scoped("boom", ToolSet::Allow(vec!["fs.list".into()])),
        ],
        FailureMode::Fallback(Box::new(fallback)),
    );

    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Allow(vec!["fs.list".into()]);
    let a = agent(&main, policy);

    let result = PipelineRunner::new()
        .run_with_delegation_depth(&main, &a, json!({}), 0, "root".into())
        .await
        .expect("fallback should rescue the pipeline");

    let inherited = observed
        .lock()
        .unwrap()
        .expect("fallback step must have run");
    assert_eq!(
        inherited, 1,
        "fallback must observe the parent's already-spent budget (1 tool call), not a fresh one"
    );

    assert_eq!(
        result.budget.tool_calls_used, 2,
        "spend inside the fallback must accumulate on top of the parent's spend \
         (1 parent + 1 fallback = 2), not restart from zero"
    );
}

/// C4.3: a fallback must not launder delegation depth. With the parent already at
/// `max_delegation_depth`, a delegation-like descent inside the fallback must be
/// blocked — before the fix the fallback restarted at depth 0 and it succeeded.
#[tokio::test]
async fn fallback_cannot_launder_delegation_depth() {
    let leaked: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let sink = leaked.clone();

    // A SubPipeline inside the fallback: legal only if there is depth headroom.
    let inner = pipeline(
        "inner",
        vec![step(
            "should_not_run",
            StepAction::Custom(Arc::new(move |_ctx| {
                *sink.lock().unwrap() = true;
                Ok(StepOutput::new("ok".into()))
            })),
            ToolSet::None,
        )],
        FailureMode::Abort,
    );

    let fallback = pipeline(
        "fallback",
        vec![step(
            "descend",
            StepAction::SubPipeline(Box::new(inner)),
            ToolSet::None,
        )],
        FailureMode::Abort,
    );

    let main = pipeline(
        "main",
        vec![failing_step("boom")],
        FailureMode::Fallback(Box::new(fallback)),
    );

    let mut policy = AgentPolicy::default();
    policy.max_delegation_depth = 2;
    let a = agent(&main, policy);

    // Parent is already AT the cap. The fallback inherits depth 2, so its
    // SubPipeline would be depth 3 — over the cap, and must be refused.
    let r = PipelineRunner::new()
        .run_with_delegation_depth(&main, &a, json!({}), 2, "grandparent".into())
        .await;

    assert!(
        r.is_err(),
        "fallback at max_delegation_depth must not be able to descend further, got {r:?}"
    );
    assert!(
        !*leaked.lock().unwrap(),
        "the laundered inner step actually executed — the fallback reset delegation depth"
    );
}

/// The `Guard::MaxDelegationDepth` guard must also see the inherited depth.
#[tokio::test]
async fn max_delegation_depth_guard_sees_fallback_depth() {
    let mut guarded = step(
        "guarded",
        StepAction::Custom(Arc::new(|_ctx| Ok(StepOutput::new("ok".into())))),
        ToolSet::None,
    );
    guarded.guard_in = Guard::MaxDelegationDepth(2);

    let main = pipeline(
        "main",
        vec![failing_step("boom")],
        FailureMode::Fallback(Box::new(pipeline(
            "fallback",
            vec![guarded],
            FailureMode::Abort,
        ))),
    );

    let mut policy = AgentPolicy::default();
    policy.max_delegation_depth = 10;
    let a = agent(&main, policy);

    // Parent at depth 3 => fallback runs at 3, exceeding the guard's max of 2.
    let r = PipelineRunner::new()
        .run_with_delegation_depth(&main, &a, json!({}), 3, "grandparent".into())
        .await;

    assert!(
        r.is_err(),
        "Guard::MaxDelegationDepth(2) must fire at inherited fallback depth 3, got {r:?}"
    );
}

/// Sibling check: the parent's `network_policy` must reach the fallback too.
#[tokio::test]
async fn network_policy_survives_fallback() {
    let observed: Arc<Mutex<Option<NetworkPolicy>>> = Arc::new(Mutex::new(None));
    let sink = observed.clone();

    let fallback = pipeline(
        "fallback",
        vec![step(
            "observe",
            StepAction::Custom(Arc::new(move |ctx| {
                *sink.lock().unwrap() = Some(ctx.network_policy.clone());
                Ok(StepOutput::new("ok".into()))
            })),
            ToolSet::None,
        )],
        FailureMode::Abort,
    );

    let main = pipeline(
        "main",
        vec![failing_step("boom")],
        FailureMode::Fallback(Box::new(fallback)),
    );

    let mut policy = AgentPolicy::default();
    policy.network_policy = NetworkPolicy::AllowList(vec!["example.com".into()]);
    let a = agent(&main, policy);

    PipelineRunner::new()
        .run(&main, &a, json!({}))
        .await
        .expect("fallback should rescue the pipeline");

    let seen = observed
        .lock()
        .unwrap()
        .clone()
        .expect("fallback step must have run");
    match seen {
        NetworkPolicy::AllowList(hosts) => assert_eq!(
            hosts,
            vec!["example.com".to_string()],
            "fallback must observe the parent's allowlist"
        ),
        other => panic!("fallback reset network_policy instead of inheriting it: {other:?}"),
    }
}

/// Sibling check: `WorkspaceIsolation::TempDir` must survive the fallback boundary
/// — a relative `fs.write` inside the fallback must not land in the real repo.
#[tokio::test]
async fn tempdir_isolation_survives_fallback() {
    let marker = "fallback_isolation_probe.txt";
    let escaped = std::env::current_dir().unwrap().join(marker);
    let _ = std::fs::remove_file(&escaped);

    let fallback = pipeline(
        "fallback",
        vec![step(
            "write",
            StepAction::ToolCall {
                tool: "fs.write".into(),
                args: json!({ "path": marker, "content": "written by fallback" }),
            },
            ToolSet::Allow(vec!["fs.write".into()]),
        )],
        FailureMode::Abort,
    );

    // Scoped to permit `fs.write` because the fallback calls it and inherits this
    // step's scope. This test asserts TempDir isolation, not tool scoping.
    let main = pipeline(
        "main",
        vec![failing_step_scoped(
            "boom",
            ToolSet::Allow(vec!["fs.write".into()]),
        )],
        FailureMode::Fallback(Box::new(fallback)),
    );

    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Allow(vec!["fs.write".into()]);
    policy.filesystem_policy.workspace_isolation = WorkspaceIsolation::TempDir;
    let a = agent(&main, policy);

    PipelineRunner::new()
        .run(&main, &a, json!({}))
        .await
        .expect("fallback should rescue the pipeline");

    let leaked = escaped.exists();
    let _ = std::fs::remove_file(&escaped);
    assert!(
        !leaked,
        "fallback escaped WorkspaceIsolation::TempDir and wrote {} into the real workspace",
        escaped.display()
    );
}
