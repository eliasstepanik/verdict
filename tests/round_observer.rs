//! Integration tests for VERDICT-CHANGE-2: the per-round `RoundObserver` hook
//! on `PipelineRunner`.
//!
//! Mirrors the real-pipeline probe style of `tool_guard.rs`: builds a genuine
//! `Pipeline` + `Agent`, runs it through the real `PipelineRunner`, and proves
//! the observer can (a) abort a `ToolUseLoop` in-flight before `max_rounds`,
//! (b) leave a loop with no observer configured completely unaffected, and
//! (c) inject a nudge that genuinely reaches the next round's LLM call.

mod common;

use async_trait::async_trait;
use common::{ScriptedMockLlmProvider, ScriptedResponse};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use verdict::prelude::*;

fn tool_use_loop_step(
    name: &str,
    tools: Vec<String>,
    max_rounds: usize,
    stop_condition: StopCondition,
) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::ToolUseLoop {
            system: "You are a tireless worker.".into(),
            user: "Keep calling the tool until told to stop.".into(),
            model: ProviderSpec {
                model: "scripted".to_string(),
                provider: "test".to_string(),
            },
            tools,
            max_rounds,
            stop_condition,
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

fn make_agent(pipeline: Pipeline) -> Agent {
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = ToolSet::Full;
    Agent {
        name: "round_observer_test_agent".into(),
        description: "round observer probe".into(),
        pipeline,
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    }
}

fn make_pipeline(step: AgentStep) -> Pipeline {
    Pipeline {
        name: "round_observer_probe".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

/// A script that always returns a tool call, never stopping naturally.
/// `n` responses are pre-loaded; `ScriptedMockLlmProvider` returns a default
/// text response ("Done.") once the script is exhausted, but with a large
/// enough `n` and a small `max_rounds`/abort-round the test never reaches it.
fn infinite_tool_call_script(n: usize) -> Vec<ScriptedResponse> {
    (0..n)
        .map(|_| ScriptedResponse::tool_call("fs.list", json!({ "path": "." })))
        .collect()
}

// ═══════════════════════════════════════════════════════════════════════════
// (a) RoundControl::Abort genuinely stops the loop early
// ═══════════════════════════════════════════════════════════════════════════

/// Observer that aborts on a specific round index.
struct AbortAtRoundObserver {
    abort_round: usize,
    reason: &'static str,
    rounds_seen: Arc<AtomicUsize>,
}

#[async_trait]
impl RoundObserver for AbortAtRoundObserver {
    async fn on_round(&self, round: usize, _history: &MessageHistory) -> RoundControl {
        self.rounds_seen.fetch_max(round + 1, Ordering::SeqCst);
        if round == self.abort_round {
            RoundControl::Abort {
                reason: self.reason.to_string(),
            }
        } else {
            RoundControl::Continue
        }
    }
}

#[tokio::test]
async fn test_round_observer_aborts_loop_before_max_rounds() {
    // The LLM always returns tool calls, so without the observer the loop
    // would run all the way to max_rounds (20). The observer aborts at
    // round 3, so the loop must stop there — not at round 4, not at 20.
    //
    // The step's LLM-facing tool list is left empty (`vec![]`) so that
    // `tool_schemas.is_empty()` holds after the abort `break`, which causes
    // `handle_tool_use_loop`'s post-loop synthesis pass to be skipped
    // (`!tool_schemas.is_empty() && !answered_with_text` gates synthesis).
    // This isolates the assertion to the abort path itself rather than
    // conflating it with the separate (pre-existing, unmodified) synthesis
    // behavior. Execution permission for `fs.list` still comes from the
    // AgentStep's own `tools: ToolSet::Full` scope, independent of this list.
    let script = infinite_tool_call_script(20);
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step("spin", vec![], 20, StopCondition::MaxRounds);
    let pipeline = make_pipeline(step);
    let agent = make_agent(pipeline.clone());

    let rounds_seen = Arc::new(AtomicUsize::new(0));
    let observer = Arc::new(AbortAtRoundObserver {
        abort_round: 3,
        reason: "test abort",
        rounds_seen: rounds_seen.clone(),
    });

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner
        .with_llm_client(Arc::new(llm_client))
        .with_round_observer(observer);

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert_eq!(
        rounds_seen.load(Ordering::SeqCst),
        4,
        "observer should have seen rounds 0,1,2,3 (aborting on round 3) — got {} rounds seen",
        rounds_seen.load(Ordering::SeqCst)
    );
    assert!(result.success, "aborted step still completes the pipeline step (clean early exit)");
    assert!(
        result.step_results["spin"].output.raw.contains("test abort"),
        "expected the abort reason to surface in the step output, got: {}",
        result.step_results["spin"].output.raw
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (b) No observer configured (default None) — behaves exactly as before
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_no_round_observer_configured_behaves_as_before() {
    // Natural stop: tool call, then text (Pattern stop condition met on round 2).
    let script = vec![
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        ScriptedResponse::text("All done. <TASK_COMPLETE>"),
    ];
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "no_observer",
        vec!["fs.list".to_string()],
        10,
        StopCondition::Pattern("<TASK_COMPLETE>".to_string()),
    );
    let pipeline = make_pipeline(step);
    let agent = make_agent(pipeline.clone());

    // No .with_round_observer(..) call at all — round_observer stays None.
    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success, "unobserved pipeline should succeed normally, got: {:?}", result);
    assert!(
        result.step_results["no_observer"]
            .output
            .raw
            .contains("<TASK_COMPLETE>"),
        "loop should reach its natural Pattern stop condition unaffected, got: {}",
        result.step_results["no_observer"].output.raw
    );
    assert!(
        !result.step_results["no_observer"].output.raw.contains("[aborted:"),
        "no observer means no abort marker should ever appear"
    );
}

/// Also prove an observer-free run that would otherwise hit max_rounds still
/// runs all the way to max_rounds with zero interference.
#[tokio::test]
async fn test_no_round_observer_runs_to_max_rounds_unaffected() {
    let script = infinite_tool_call_script(5);
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "spin_no_observer",
        vec!["fs.list".to_string()],
        5,
        StopCondition::MaxRounds,
    );
    let pipeline = make_pipeline(step);
    let agent = make_agent(pipeline.clone());

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert!(
        !result.step_results["spin_no_observer"].output.raw.contains("[aborted:"),
        "no observer configured — must never see an abort marker"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (c) pending_nudge() genuinely injects a message the next round's LLM call sees
// ═══════════════════════════════════════════════════════════════════════════

/// Observer that emits a fixed nudge exactly once, after round 0, and never aborts.
struct OneShotNudgeObserver {
    nudge_text: &'static str,
    nudge_sent: Mutex<bool>,
}

#[async_trait]
impl RoundObserver for OneShotNudgeObserver {
    async fn on_round(&self, round: usize, _history: &MessageHistory) -> RoundControl {
        // Arm the nudge after round 0 completes (i.e. this call is for round 1).
        if round == 1 {
            *self.nudge_sent.lock().unwrap() = true;
        }
        RoundControl::Continue
    }

    fn pending_nudge(&self) -> Option<String> {
        if *self.nudge_sent.lock().unwrap() {
            Some(self.nudge_text.to_string())
        } else {
            None
        }
    }
}

#[tokio::test]
async fn test_pending_nudge_reaches_next_round_llm_call() {
    let script = vec![
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })), // round 0
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })), // round 1
        ScriptedResponse::text("Done. <TASK_COMPLETE>"),                // round 2
    ];
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let captured_requests = mock_provider.captured_requests.clone();
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "nudge_test",
        vec!["fs.list".to_string()],
        10,
        StopCondition::Pattern("<TASK_COMPLETE>".to_string()),
    );
    let pipeline = make_pipeline(step);
    let agent = make_agent(pipeline.clone());

    let observer = Arc::new(OneShotNudgeObserver {
        nudge_text: "NUDGE: you seem stuck, try a different approach",
        nudge_sent: Mutex::new(false),
    });

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner
        .with_llm_client(Arc::new(llm_client))
        .with_round_observer(observer);

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();
    assert!(result.success);

    let requests = captured_requests.lock().unwrap();
    // Round index 2 (third LLM call) is the one issued AFTER on_round(1, ..)
    // armed the nudge and pending_nudge() pushed it into history.
    let round_2_request = requests
        .get(2)
        .expect("expected at least 3 LLM calls (rounds 0, 1, 2)");
    let history = round_2_request
        .history
        .as_ref()
        .expect("round 2 request should carry conversation history");

    let nudge_present = history
        .messages
        .iter()
        .any(|m| m.content.contains("NUDGE: you seem stuck"));

    assert!(
        nudge_present,
        "expected the nudge text to appear in round 2's request history, got messages: {:?}",
        history.messages.iter().map(|m| &m.content).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// (d) VERDICT-CHANGE-3 adversarial regression: Abort must gate OUT the
//     post-main-loop synthesis pass, not just the main loop itself.
//
// This reproduces the exact bug found by the debugger and documented in
// notes/verdict-round-observer-synthesis-pass-bypass.md: a REALISTIC
// non-empty `tools` config (the entire point of `ToolUseLoop`), where the
// pre-fix code would let `run_synthesis_loop` fire up to 10 more unobserved
// LLM rounds after the abort `break`, silently overwriting the
// `"[aborted: ...]"` marker in the process. Before the fix in
// `src/runner/tool_use_loop.rs` (the `aborted_by_observer` gate on entry
// into `run_synthesis_loop`), this test would have observed 13 total LLM
// calls and an empty step output with no abort marker; after the fix it
// must observe exactly 3 calls and a preserved abort marker.
// ═══════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_abort_skips_synthesis_pass_with_realistic_nonempty_tools() {
    // The LLM never stops naturally (always returns a tool call) — 30
    // scripted responses is far more than the 4 the loop should ever reach.
    let script = infinite_tool_call_script(30);
    let mock_provider = ScriptedMockLlmProvider::new(script);
    let captured_requests = mock_provider.captured_requests.clone();
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    // REALISTIC config: non-empty `tools` list — this is what every real
    // ToolUseLoop step looks like, and exactly the config the pre-fix code
    // mishandled (an empty `tools` list was the only way the shipped 4 tests
    // avoided tripping over this gap).
    let step = tool_use_loop_step("spin_realistic", vec!["fs.list".to_string()], 20, StopCondition::MaxRounds);
    let pipeline = make_pipeline(step);
    let agent = make_agent(pipeline.clone());

    let rounds_seen = Arc::new(AtomicUsize::new(0));
    let observer = Arc::new(AbortAtRoundObserver {
        abort_round: 3,
        reason: "test abort",
        rounds_seen: rounds_seen.clone(),
    });

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner
        .with_llm_client(Arc::new(llm_client))
        .with_round_observer(observer);

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    // (a) on_round() was called exactly 4 times: rounds 0, 1, 2, 3 (aborting
    // on round 3's poll, before round 3's LLM call is ever issued).
    assert_eq!(
        rounds_seen.load(Ordering::SeqCst),
        4,
        "observer should have been polled for rounds 0,1,2,3 (aborting on round 3) — got {} rounds seen",
        rounds_seen.load(Ordering::SeqCst)
    );

    // (b) Total LLM call count is bounded to ONLY the main-loop calls that
    // happened before the abort: rounds 0, 1, 2 → 3 calls. Pre-fix this was
    // 13 (3 main-loop + up to 10 unobserved synthesis-loop calls).
    let total_llm_calls = captured_requests.lock().unwrap().len();
    assert_eq!(
        total_llm_calls, 3,
        "expected exactly 3 LLM calls (rounds 0,1,2 before the round-3 abort), got {} — \
         extra calls indicate the synthesis pass ran after the abort",
        total_llm_calls
    );

    // (c) The step output DOES contain the "[aborted: ...]" marker with the
    // correct reason text. Pre-fix, run_synthesis_loop's return value
    // unconditionally overwrote final_text, discarding this marker.
    assert!(result.success, "aborted step still completes the pipeline step (clean early exit)");
    let output = &result.step_results["spin_realistic"].output.raw;
    assert!(
        output.contains("[aborted: test abort]"),
        "expected the abort marker to survive (not be overwritten by a skipped synthesis pass), got: {}",
        output
    );

    // (d) No synthesis-loop calls happened at all: since every call made is
    // accounted for by the main loop (3, matching (b) exactly), the
    // synthesis-specific call count is exactly 0.
    let synthesis_calls = total_llm_calls.saturating_sub(3);
    assert_eq!(
        synthesis_calls, 0,
        "expected zero synthesis-loop LLM calls after an explicit abort, got {}",
        synthesis_calls
    );
}
