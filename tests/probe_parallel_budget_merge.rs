//! REAL-PIPELINE PROBE: parallel steps must not hide their spend.
//!
//! Parallel steps execute on isolated `StepContext` clones. Before the
//! `merge_parallel_step_deltas` fix, only `step_results` came back — the per-step
//! budget spend and the `tools_used` / `commands_executed` records were dropped
//! with the clone. That made `Guard::MaxToolCalls` / `Guard::MaxCostUsd` see zero
//! spend and made every shell command run by a parallel step invisible to
//! `ShellCommandAllowlist` / `ShellCommandDenylist`.
//!
//! These are NOT fixture tests: each builds a genuine `Pipeline` + `Agent` and
//! runs it through the real `PipelineRunner` against the real builtin
//! `ToolRegistry`. Nothing here writes `budget` or `commands_executed` by hand.

use serde_json::json;
use verdict::prelude::*;

/// A step that really invokes `shell.run_command`.
fn shell_step(name: &str, command: &str, parallel: bool, guard_out: Guard) -> AgentStep {
    shell_step_with_arg(name, command, "parallel_probe_arg", parallel, guard_out)
}

/// Same, with an explicit argument — used where the command has a filesystem
/// side effect that the test must clean up.
fn shell_step_with_arg(
    name: &str,
    command: &str,
    arg: &str,
    parallel: bool,
    guard_out: Guard,
) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall {
            tool: "shell.run_command".into(),
            args: json!({ "command": command, "args": [arg] }),
        },
        guard_out,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel,
        input_processors: vec![],
        output_processors: vec![],
    }
}

/// Run a pipeline with an agent policy permissive enough that the tools really
/// execute — the step scope is intersected with the AGENT policy scope, which
/// defaults to `None`, so without this nothing would run and every assertion
/// would pass for the wrong reason.
async fn run(pipeline: Pipeline) -> Result<PipelineResult, PipelineError> {
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Full;

    let agent = Agent {
        name: "parallel_probe_agent".into(),
        description: "parallel probe".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    };
    PipelineRunner::new().run(&pipeline, &agent, json!({})).await
}

/// PROBE P1: two parallel steps each make one real tool call. The PARENT budget
/// must show the SUM (2), not 0 and not 1.
#[tokio::test]
async fn probe_parallel_tool_calls_sum_into_parent_budget() {
    let pipeline = Pipeline {
        name: "parallel_budget_probe".into(),
        steps: vec![
            shell_step("par_a", "echo", true, Guard::None),
            shell_step("par_b", "echo", true, Guard::None),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let result = run(pipeline).await.expect("parallel batch should succeed");

    println!("P1 tool_calls_used = {}", result.budget.tool_calls_used);
    println!("P1 llm_calls_used  = {}", result.budget.llm_calls_used);
    println!("P1 steps_passed    = {:?}", result.steps_passed);

    assert_eq!(
        result.steps_passed.len(),
        2,
        "both parallel steps must actually have run: {:?}",
        result.steps_passed
    );
    assert_eq!(
        result.budget.tool_calls_used, 2,
        "parent budget must reflect the SUM of both parallel steps' tool calls, \
         got {} (0 => deltas discarded, 1 => only one step merged)",
        result.budget.tool_calls_used
    );
}

/// PROBE P2: three parallel tool calls sum to 3 — proves the merge scales and is
/// not a hardcoded "one extra".
#[tokio::test]
async fn probe_three_parallel_tool_calls_sum_to_three() {
    let pipeline = Pipeline {
        name: "parallel_budget_probe_3".into(),
        steps: vec![
            shell_step("par_a", "echo", true, Guard::None),
            shell_step("par_b", "echo", true, Guard::None),
            shell_step("par_c", "echo", true, Guard::None),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let result = run(pipeline).await.expect("parallel batch should succeed");

    println!("P2 tool_calls_used = {}", result.budget.tool_calls_used);
    assert_eq!(
        result.budget.tool_calls_used, 3,
        "three parallel tool calls must sum to 3, got {}",
        result.budget.tool_calls_used
    );
}

/// PROBE P3: a `MaxToolCalls` guard on a step FOLLOWING the parallel batch must
/// be able to see the parallel spend and trip on it.
#[tokio::test]
async fn probe_max_tool_calls_guard_sees_parallel_spend() {
    let pipeline = Pipeline {
        name: "parallel_max_tool_calls_probe".into(),
        steps: vec![
            shell_step("par_a", "echo", true, Guard::None),
            shell_step("par_b", "echo", true, Guard::None),
            // Budget after the batch is 2 tool calls + this step's own = 3 > 1.
            shell_step("after", "echo", false, Guard::MaxToolCalls(1)),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let result = run(pipeline).await;
    println!("P3 result = {:?}", result);

    match &result {
        Err(PipelineError::GuardFailed { error, .. }) => {
            let msg = format!("{:?}", error);
            assert!(
                msg.contains("MaxToolCalls"),
                "blocked, but not by MaxToolCalls: {}",
                msg
            );
        }
        other => panic!(
            "BYPASS: MaxToolCalls(1) did not trip after 2 parallel tool calls \
             plus one sequential call. result={:?}",
            other
        ),
    }
}

/// PROBE P4: a shell command executed by a PARALLEL step must be visible to a
/// `ShellCommandDenylist` guard on a LATER step. This is the guard-blindness
/// half of the bug: before the fix, `commands_executed` from the parallel batch
/// never reached the parent, so the denylist had nothing to match against.
#[tokio::test]
async fn probe_denylist_sees_parallel_step_commands() {
    // `touch` really creates a file, so the target is pid-unique and removed
    // below — otherwise the probe litters the worktree. It must be
    // workspace-RELATIVE: an absolute /tmp path is rejected by the tool's
    // workspace-containment check before guards ever run, which would make this
    // probe pass for the wrong reason.
    let canary_rel = format!("parallel_canary_{}.txt", std::process::id());
    let canary_abs = std::env::current_dir().unwrap().join(&canary_rel);
    let _ = std::fs::remove_file(&canary_abs);

    let pipeline = Pipeline {
        name: "parallel_denylist_probe".into(),
        steps: vec![
            // The denylisted command runs INSIDE a parallel step.
            shell_step_with_arg("par_touch", "touch", &canary_rel, true, Guard::None),
            shell_step("par_echo", "echo", true, Guard::None),
            // A later sequential step denies "touch". It can only fire if the
            // parallel step's command was merged into the parent context.
            shell_step(
                "after",
                "echo",
                false,
                Guard::ShellCommandDenylist(vec!["touch".to_string()]),
            ),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let result = run(pipeline).await;
    let canary_created = canary_abs.exists();
    let _ = std::fs::remove_file(&canary_abs);

    println!("P4 result = {:?}", result);
    println!("P4 canary_exists = {}", canary_created);
    assert!(
        canary_created,
        "the parallel 'touch' step must really have executed, otherwise the \
         denylist assertion below would pass for the wrong reason"
    );

    match &result {
        Err(PipelineError::GuardFailed { error, .. }) => {
            let msg = format!("{:?}", error);
            assert!(
                msg.contains("ShellCommandDenylist"),
                "blocked, but not by the denylist guard: {}",
                msg
            );
            assert!(
                msg.contains("touch"),
                "denylist fired on the wrong command — expected the parallel \
                 step's 'touch' to be the match: {}",
                msg
            );
        }
        other => panic!(
            "BYPASS: 'touch' executed by a PARALLEL step was invisible to a \
             later ShellCommandDenylist(['touch']). result={:?}",
            other
        ),
    }
}

/// CONTROL: the denylist must NOT fire when the parallel steps ran nothing
/// denylisted. Without this, P4 could pass by blanket-failing.
#[tokio::test]
async fn probe_control_denylist_does_not_overfire() {
    let pipeline = Pipeline {
        name: "parallel_denylist_control".into(),
        steps: vec![
            shell_step("par_a", "echo", true, Guard::None),
            shell_step("par_b", "echo", true, Guard::None),
            shell_step(
                "after",
                "echo",
                false,
                Guard::ShellCommandDenylist(vec!["rm".to_string()]),
            ),
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let result = run(pipeline).await;
    println!("CONTROL result = {:?}", result);
    assert!(
        result.is_ok(),
        "FALSE POSITIVE: denylist ['rm'] should not block echo-only steps. result={:?}",
        result
    );

    let result = result.unwrap();
    assert_eq!(
        result.budget.tool_calls_used, 3,
        "2 parallel + 1 sequential tool call must total 3, got {}",
        result.budget.tool_calls_used
    );
}
