//! Regression: `StepAction::SubPipeline` is a delegation boundary and must
//! inherit the parent's security context.
//!
//! The handler used to build a fresh `AgentPolicy::default()` (copying only
//! `allowed_tools`) and call the plain `run()` entry point. That silently reset
//! `filesystem_policy` to a `current_dir()`-based root — so an inner `fs.write`
//! escaped `WorkspaceIsolation::TempDir` and landed in the real repository —
//! reset `network_policy` to `DenyAll`, and reset `delegation_depth` to 0,
//! letting a SubPipeline wrapper bypass `max_delegation_depth` entirely.
//!
//! Each test observes a real effect (a file's actual location on disk, the depth
//! value seen by an inner step), not just an internal assertion.

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

fn pipeline(name: &str, steps: Vec<AgentStep>) -> Pipeline {
    Pipeline {
        name: name.into(),
        steps,
        on_failure: FailureMode::Abort,
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

/// C2: `WorkspaceIsolation::TempDir` must survive the SubPipeline boundary.
///
/// The inner step writes via `fs.write` with a relative path. If isolation is
/// inherited the file lands under the temp workspace; if the child rebuilt a
/// default policy it lands in the process's real current directory (the repo).
#[tokio::test]
async fn tempdir_isolation_survives_subpipeline() {
    let marker = "subpipeline_isolation_probe.txt";

    // Guarantee a clean slate, and make sure we observe the real cwd.
    let escaped = std::env::current_dir().unwrap().join(marker);
    let _ = std::fs::remove_file(&escaped);

    let inner = pipeline(
        "inner",
        vec![step(
            "write",
            StepAction::ToolCall {
                tool: "fs.write".into(),
                args: json!({ "path": marker, "content": "written by sub-pipeline" }),
            },
            ToolSet::Allow(vec!["fs.write".into()]),
        )],
    );

    let outer = pipeline(
        "outer",
        vec![step(
            "sub",
            StepAction::SubPipeline(Box::new(inner)),
            ToolSet::Allow(vec!["fs.write".into()]),
        )],
    );

    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Allow(vec!["fs.write".into()]);
    policy.filesystem_policy.workspace_isolation = WorkspaceIsolation::TempDir;

    let a = agent(&outer, policy);
    let result = PipelineRunner::new()
        .run(&outer, &a, json!({}))
        .await
        .expect("pipeline should run");
    assert!(result.success, "pipeline should succeed: {result:?}");

    // The whole point: the write must NOT have escaped into the real workspace.
    let leaked = escaped.exists();
    let _ = std::fs::remove_file(&escaped);
    assert!(
        !leaked,
        "SubPipeline escaped WorkspaceIsolation::TempDir and wrote {} into the real workspace",
        escaped.display()
    );
}

/// C3: `delegation_depth` must be incremented, not reset, across the boundary.
#[tokio::test]
async fn delegation_depth_increments_through_subpipeline() {
    let observed: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let sink = observed.clone();

    let inner = pipeline(
        "inner",
        vec![step(
            "observe",
            StepAction::Custom(Arc::new(move |ctx| {
                *sink.lock().unwrap() = Some(ctx.delegation_depth);
                Ok(StepOutput::new("ok".into()))
            })),
            ToolSet::None,
        )],
    );

    let outer = pipeline(
        "outer",
        vec![step(
            "sub",
            StepAction::SubPipeline(Box::new(inner)),
            ToolSet::None,
        )],
    );

    let a = agent(&outer, AgentPolicy::default());

    // Enter the parent at depth 3, as a delegated agent would be.
    PipelineRunner::new()
        .run_with_delegation_depth(&outer, &a, json!({}), 3, "grandparent".into())
        .await
        .expect("pipeline should run");

    let depth = observed.lock().unwrap().expect("inner step must have run");
    assert_eq!(
        depth, 4,
        "inner SubPipeline step must observe parent depth + 1 (4), not a reset depth"
    );
}

/// C3: the depth cap must actually block, so SubPipeline can't launder delegation.
#[tokio::test]
async fn subpipeline_cannot_bypass_max_delegation_depth() {
    let inner = pipeline(
        "inner",
        vec![step(
            "noop",
            StepAction::Custom(Arc::new(|_ctx| Ok(StepOutput::new("ok".into())))),
            ToolSet::None,
        )],
    );

    let outer = pipeline(
        "outer",
        vec![step(
            "sub",
            StepAction::SubPipeline(Box::new(inner)),
            ToolSet::None,
        )],
    );

    let mut policy = AgentPolicy::default();
    policy.max_delegation_depth = 2;
    let a = agent(&outer, policy);

    // Parent already at the cap: descending one more level must be refused.
    let r = PipelineRunner::new()
        .run_with_delegation_depth(&outer, &a, json!({}), 2, "grandparent".into())
        .await;

    assert!(
        r.is_err(),
        "SubPipeline at depth 2 with max_delegation_depth=2 must be blocked, got {r:?}"
    );
}

/// The `Guard::MaxDelegationDepth` guard must see the incremented depth too.
#[tokio::test]
async fn max_delegation_depth_guard_sees_subpipeline_depth() {
    let mut guarded = step(
        "guarded",
        StepAction::Custom(Arc::new(|_ctx| Ok(StepOutput::new("ok".into())))),
        ToolSet::None,
    );
    guarded.guard_in = Guard::MaxDelegationDepth(3);

    let outer = pipeline(
        "outer",
        vec![step(
            "sub",
            StepAction::SubPipeline(Box::new(pipeline("inner", vec![guarded]))),
            ToolSet::None,
        )],
    );

    let a = agent(&outer, AgentPolicy::default());

    // Parent at depth 3 => inner runs at 4, which exceeds the guard's max of 3.
    let r = PipelineRunner::new()
        .run_with_delegation_depth(&outer, &a, json!({}), 3, "grandparent".into())
        .await;

    assert!(
        r.is_err(),
        "Guard::MaxDelegationDepth(3) must fire at inner depth 4, got {r:?}"
    );
}

/// H2: `network_policy` must be inherited, not reset to `DenyAll`.
#[tokio::test]
async fn network_policy_survives_subpipeline() {
    let observed: Arc<Mutex<Option<NetworkPolicy>>> = Arc::new(Mutex::new(None));
    let sink = observed.clone();

    let inner = pipeline(
        "inner",
        vec![step(
            "observe",
            StepAction::Custom(Arc::new(move |ctx| {
                *sink.lock().unwrap() = Some(ctx.network_policy.clone());
                Ok(StepOutput::new("ok".into()))
            })),
            ToolSet::None,
        )],
    );

    let outer = pipeline(
        "outer",
        vec![step(
            "sub",
            StepAction::SubPipeline(Box::new(inner)),
            ToolSet::None,
        )],
    );

    let mut policy = AgentPolicy::default();
    policy.network_policy = NetworkPolicy::AllowList(vec!["example.com".into()]);
    let a = agent(&outer, policy);

    PipelineRunner::new()
        .run(&outer, &a, json!({}))
        .await
        .expect("pipeline should run");

    let seen = observed.lock().unwrap().clone().expect("inner step must have run");
    match seen {
        NetworkPolicy::AllowList(hosts) => {
            assert_eq!(
                hosts,
                vec!["example.com".to_string()],
                "inner step must observe the parent's allowlist"
            );
        }
        other => panic!(
            "SubPipeline reset network_policy instead of inheriting it: {other:?}"
        ),
    }
}

/// Budget accounting must be continuous across the boundary, not restarted.
#[tokio::test]
async fn budget_is_inherited_through_subpipeline() {
    let observed: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
    let sink = observed.clone();

    let inner = pipeline(
        "inner",
        vec![step(
            "observe",
            StepAction::Custom(Arc::new(move |ctx| {
                *sink.lock().unwrap() = Some(ctx.budget.tool_calls_used);
                Ok(StepOutput::new("ok".into()))
            })),
            ToolSet::None,
        )],
    );

    // A real tool call in the parent increments `tool_calls_used`; the inner
    // step must observe that spend rather than a zeroed budget.
    let bump = step(
        "bump",
        StepAction::ToolCall {
            tool: "fs.list".into(),
            args: json!({ "path": "." }),
        },
        ToolSet::Allow(vec!["fs.list".into()]),
    );

    let outer = pipeline(
        "outer",
        vec![
            bump,
            step(
                "sub",
                StepAction::SubPipeline(Box::new(inner)),
                ToolSet::None,
            ),
        ],
    );

    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Allow(vec!["fs.list".into()]);
    let a = agent(&outer, policy);
    PipelineRunner::new()
        .run(&outer, &a, json!({}))
        .await
        .expect("pipeline should run");

    assert!(
        observed.lock().unwrap().expect("inner step must have run") > 0,
        "SubPipeline must inherit the parent's budget (tool_calls_used), not start a fresh one"
    );
}
