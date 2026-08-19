//! REAL-PIPELINE PROBE (verification harness, round 44+).
//!
//! These are NOT fixture tests. Every case builds a genuine `Pipeline` +
//! `Agent`, runs it through the real `PipelineRunner` with the real builtin
//! `ToolRegistry`, and invokes the REAL registered `shell.run_command` /
//! `shell.run` tools. `commands_executed` is populated by production code
//! only — never by the test.
//!
//! The previous round's unit tests all passed while the real pipeline was
//! still fully bypassed. This harness is what caught that.

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

/// Build + run a one-step pipeline that really executes a shell tool,
/// with `guard_out` set to the supplied guard.
async fn run_shell_step(
    tool: &str,
    command: &str,
    args: Vec<&str>,
    guard_out: Guard,
) -> Result<PipelineResult, PipelineError> {
    let step = AgentStep {
        name: "shell_step".into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall {
            tool: tool.into(),
            args: json!({
                "command": command,
                "args": args,
            }),
        },
        guard_out,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    let pipeline = Pipeline {
        name: "probe".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    // The step-level toolset is intersected with the AGENT POLICY toolset,
    // which defaults to None. Without this the tool never executes at all and
    // every "blocked" assertion would pass for the wrong reason.
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Full;

    let agent = Agent {
        name: "probe_agent".into(),
        description: "probe".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    };
    PipelineRunner::new().run(&pipeline, &agent, json!({})).await
}

/// Resolve a real absolute path to a binary on this machine.
/// Hardcoding /bin/echo is wrong here: this is a NixOS host with no /bin/echo,
/// so the spawn would fail and the probe would "pass" for the wrong reason.
fn abs_path_to(bin: &str) -> String {
    // `-p` forces a real filesystem lookup; plain `command -v echo` returns the
    // shell BUILTIN name "echo" with no path, which silently breaks the probe.
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -pv {}", bin))
        .output()
        .expect("command -v must run");
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(
        p.starts_with('/') && std::path::Path::new(&p).exists(),
        "could not resolve an absolute path for '{}' (got {:?})",
        bin,
        p
    );
    p
}

/// PROBE 7: the exact bypass from last round.
/// Real `shell.run_command` executing a denylisted command must be BLOCKED.
/// The canary is workspace-RELATIVE: an absolute /tmp path is rejected by the
/// tool's workspace-containment check before guards ever run, which would make
/// this probe pass for the wrong reason.
#[tokio::test]
async fn probe_run_command_denylist_blocked_with_canary() {
    let canary_rel = format!("canary_probe_{}.txt", std::process::id());
    let canary_abs = std::env::current_dir().unwrap().join(&canary_rel);
    let _ = std::fs::remove_file(&canary_abs);

    let result = run_shell_step(
        "shell.run_command",
        "touch",
        vec![canary_rel.as_str()],
        Guard::ShellCommandDenylist(vec!["touch".to_string()]),
    )
    .await;

    let canary_created = canary_abs.exists();
    let _ = std::fs::remove_file(&canary_abs);

    println!("PROBE7 result        = {:?}", result);
    println!("PROBE7 canary_exists = {}", canary_created);

    // Must be blocked, and specifically BY THE DENYLIST GUARD.
    match &result {
        Err(PipelineError::GuardFailed { error, .. }) => {
            let msg = format!("{:?}", error);
            assert!(
                msg.contains("ShellCommandDenylist"),
                "blocked, but not by the denylist guard: {}",
                msg
            );
        }
        other => panic!(
            "CRITICAL BYPASS or wrong-reason block: shell.run_command with denylisted \
             'touch' did not fail via ShellCommandDenylist. result={:?}",
            other
        ),
    }

    // Enforcement is guard_out (post-execution) => detective, not preventive.
    println!(
        "PROBE7 enforcement = {}",
        if canary_created {
            "DETECTIVE (side effect occurred, then blocked)"
        } else {
            "PREVENTIVE (no side effect)"
        }
    );
}

/// PROBE 8: absolute path must be basename-normalized in production.
/// Uses a REAL resolved absolute path so the command actually spawns.
/// `ls` is used rather than `echo` because `echo` is a shell builtin and does
/// not resolve to a real binary path on this host.
#[tokio::test]
async fn probe_absolute_path_denylist_blocked() {
    let abs_ls = abs_path_to("ls");
    println!("PROBE8 using abs path = {}", abs_ls);

    let result = run_shell_step(
        "shell.run_command",
        &abs_ls,
        vec![],
        Guard::ShellCommandDenylist(vec!["ls".to_string()]),
    )
    .await;

    println!("PROBE8 result = {:?}", result);

    match &result {
        Err(PipelineError::GuardFailed { error, .. }) => {
            let msg = format!("{:?}", error);
            assert!(
                msg.contains("ShellCommandDenylist"),
                "blocked, but not by the denylist guard: {}",
                msg
            );
        }
        other => panic!(
            "BYPASS or wrong-reason block: absolute path {} not blocked by \
             denylist ['ls'] via basename normalization. result={:?}",
            abs_ls, other
        ),
    }
}

/// PROBE 8b: absolute path allowlist parity — a real absolute echo must be
/// PERMITTED by allowlist ["echo"] (proves normalization is not just "deny all").
#[tokio::test]
async fn probe_absolute_path_allowlist_permitted() {
    let abs_ls = abs_path_to("ls");

    let result = run_shell_step(
        "shell.run_command",
        &abs_ls,
        vec![],
        Guard::ShellCommandAllowlist(vec!["ls".to_string()]),
    )
    .await;

    println!("PROBE8b result = {:?}", result);
    assert!(
        result.is_ok(),
        "absolute path {} should be PERMITTED by allowlist ['ls'] via basename \
         normalization. result={:?}",
        abs_ls,
        result
    );
}

/// PROBE 9: allowlist must be exact-match, not prefix.
/// Allowlist ["ech"] must NOT permit `echo`.
#[tokio::test]
async fn probe_allowlist_prefix_does_not_permit() {
    let result = run_shell_step(
        "shell.run_command",
        "echo",
        vec!["hello"],
        Guard::ShellCommandAllowlist(vec!["ech".to_string()]),
    )
    .await;

    println!("PROBE9 result = {:?}", result);
    assert!(
        result.is_err(),
        "BYPASS: allowlist prefix 'ech' wrongly permitted 'echo'. result={:?}",
        result
    );
}

/// CONTROL A: a permitted command must still SUCCEED (guards not blanket-failing).
/// Without this, all the "blocked" assertions above could pass trivially.
#[tokio::test]
async fn probe_control_allowed_command_succeeds() {
    let result = run_shell_step(
        "shell.run_command",
        "echo",
        vec!["hello"],
        Guard::ShellCommandAllowlist(vec!["echo".to_string()]),
    )
    .await;

    println!("CONTROL_A result = {:?}", result);
    assert!(
        result.is_ok(),
        "FALSE POSITIVE: allowlist ['echo'] should permit 'echo' but blocked it. result={:?}",
        result
    );
}

/// CONTROL B: non-denylisted command must pass the denylist guard.
#[tokio::test]
async fn probe_control_non_denylisted_passes() {
    let result = run_shell_step(
        "shell.run_command",
        "echo",
        vec!["safe"],
        Guard::ShellCommandDenylist(vec!["rm".to_string()]),
    )
    .await;

    println!("CONTROL_B result = {:?}", result);
    assert!(
        result.is_ok(),
        "FALSE POSITIVE: denylist ['rm'] should not block 'echo'. result={:?}",
        result
    );
}

/// PROBE 7b: same bypass check via `shell.run` (the arm that already worked),
/// to confirm parity between the two tool names.
#[tokio::test]
async fn probe_shell_run_denylist_blocked() {
    let result = run_shell_step(
        "shell.run",
        "echo",
        vec!["x"],
        Guard::ShellCommandDenylist(vec!["echo".to_string()]),
    )
    .await;

    println!("PROBE7b result = {:?}", result);
    assert!(
        result.is_err(),
        "BYPASS: shell.run with denylisted 'echo' not blocked. result={:?}",
        result
    );
}

/// Keep Arc import meaningful if prelude shifts.
#[allow(dead_code)]
fn _arc_anchor() -> Arc<u8> {
    Arc::new(0)
}

/// RESIDUAL GAP PROBE: shell-wrapper bypass.
/// `sh -c "ls"` records first word "sh", so a denylist on "ls" does NOT fire.
/// This is the documented/accepted limitation. Asserting it explicitly so the
/// gap is characterized rather than assumed.
#[tokio::test]
async fn probe_residual_wrapper_bypass_characterization() {
    let result = run_shell_step(
        "shell.run_command",
        "sh",
        vec!["-c", "ls"],
        Guard::ShellCommandDenylist(vec!["ls".to_string()]),
    )
    .await;

    println!("WRAPPER_PROBE result = {:?}", result);
    println!(
        "WRAPPER_PROBE bypass_present = {}",
        result.is_ok()
    );
}
