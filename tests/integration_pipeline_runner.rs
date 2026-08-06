//! Integration tests: Pipeline Runner end-to-end flows
//!
//! Tests the full pipeline execution path via PipelineRunner::run() â€”
//! not guard evaluation in isolation. All tests use StepAction::Custom
//! for deterministic, LLM-free execution unless otherwise noted.

use serde_json::json;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use verdict::prelude::*;

// â”€â”€â”€ shared helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn custom_step(
    name: &str,
    f: impl Fn(&StepContext) -> Result<StepOutput, StepError> + Send + Sync + 'static,
) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(f)),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

fn abort_pipeline(name: &str, steps: Vec<AgentStep>) -> Pipeline {
    Pipeline {
        name: name.into(),
        steps,
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

fn simple_agent(pipeline: &Pipeline) -> Agent {
    Agent {
        name: "test_agent".into(),
        description: "integration test agent".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: vec![],
    }
}

// â”€â”€â”€ Test 1: 3-step data chain via step_results â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_data_flows_step1_to_step2_to_step3_via_step_results() {
    let step1 = custom_step("step1", |_ctx| Ok(StepOutput::new("alpha".into())));

    let step2 = custom_step("step2", |ctx| {
        let prev = ctx
            .step_results
            .get("step1")
            .ok_or_else(|| StepError::ActionFailed {
                reason: "no step1".into(),
            })?
            .output
            .raw
            .clone();
        Ok(StepOutput::new(format!("{prev}-beta")))
    });

    let step3 = custom_step("step3", |ctx| {
        let prev = ctx
            .step_results
            .get("step2")
            .ok_or_else(|| StepError::ActionFailed {
                reason: "no step2".into(),
            })?
            .output
            .raw
            .clone();
        Ok(StepOutput::new(format!("{prev}-gamma")))
    });

    let pipeline = abort_pipeline("chain", vec![step1, step2, step3]);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert_eq!(result.steps_passed, vec!["step1", "step2", "step3"]);
    assert!(result.steps_failed.is_empty());
    assert_eq!(result.step_results["step1"].output.raw, "alpha");
    assert_eq!(result.step_results["step2"].output.raw, "alpha-beta");
    assert_eq!(result.step_results["step3"].output.raw, "alpha-beta-gamma");
}

// â”€â”€â”€ Test 2: Retry exhaustion returns MaxRetriesExceeded â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_retry_exhaustion_returns_max_retries_exceeded() {
    let attempts = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&attempts);

    let failing = AgentStep {
        name: "always_fails".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |_ctx| {
            counter.fetch_add(1, Ordering::SeqCst);
            Err(StepError::ActionFailed {
                reason: "always".into(),
            })
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "retry_exhaust".into(),
        steps: vec![failing],
        on_failure: FailureMode::Retry,
        max_retries: 2,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let err = runner.run(&pipeline, &agent, json!({})).await.unwrap_err();

    assert!(
        matches!(err, PipelineError::MaxRetriesExceeded { ref step } if step == "always_fails"),
        "expected MaxRetriesExceeded, got {err:?}"
    );
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        3,
        "1 initial + 2 retries = 3 attempts"
    );
}

// â”€â”€â”€ Test 3: guard_out NonEmptyOutput blocks empty step â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_guard_out_nonempty_blocks_empty_output() {
    let empty_step = AgentStep {
        name: "empty_step".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| Ok(StepOutput::new(String::new())))),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = abort_pipeline("guard_blocks", vec![empty_step]);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let err = runner.run(&pipeline, &agent, json!({})).await.unwrap_err();

    assert!(
        matches!(err, PipelineError::GuardFailed { ref step, phase: GuardPhase::Out, .. } if step == "empty_step"),
        "expected GuardFailed(Out) at empty_step, got {err:?}"
    );
}

// â”€â”€â”€ Test 4: guard_out ValidJson passes on valid JSON â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_guard_out_valid_json_passes_on_json_output() {
    let json_step = AgentStep {
        name: "json_step".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new(r#"{"status":"ok","count":42}"#.into()))
        })),
        guard_out: Guard::ValidJson,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = abort_pipeline("json_ok", vec![json_step]);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&pipeline, &agent, json!({}))
        .await
        .expect("ValidJson guard should pass on well-formed JSON");

    assert!(result.success);
    assert_eq!(
        result.step_results["json_step"].output.raw,
        r#"{"status":"ok","count":42}"#
    );
}

// â”€â”€â”€ Test 5: Verdict::None and Verdict::Automated(Guard::None) are equivalent â”€

async fn run_with_verdict(verdict: Verdict) -> PipelineResult {
    let step = AgentStep {
        name: "s".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| Ok(StepOutput::new("ok".into())))),
        guard_out: Guard::None,
        verdict,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    let pipeline = abort_pipeline("v", vec![step]);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    runner.run(&pipeline, &agent, json!({})).await.unwrap()
}

#[tokio::test]
async fn test_verdict_none_and_automated_none_are_equivalent() {
    let a = run_with_verdict(Verdict::None).await;
    let b = run_with_verdict(Verdict::Automated(Guard::None)).await;

    assert!(a.success && b.success);
    assert!(a.step_results["s"].verdict_passed);
    assert!(b.step_results["s"].verdict_passed);
    assert_eq!(a.steps_passed, b.steps_passed);
    assert_eq!(a.steps_failed, b.steps_failed);
}

// â”€â”€â”€ Test 6: Fallback runs in clean context (no prior step_results) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_fallback_pipeline_runs_in_clean_context() {
    let observed_keys: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
    let observer = Arc::clone(&observed_keys);

    let primary_ok = custom_step("primary_ok", |_| Ok(StepOutput::new("a".into())));
    let fail_step = custom_step("fail_step", |_| {
        Err(StepError::ActionFailed {
            reason: "force fallback".into(),
        })
    });

    let fb_observe = AgentStep {
        name: "fb_observe".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |ctx| {
            let count = ctx.step_results.len();
            *observer.lock().unwrap() = Some(count);
            Ok(StepOutput::new("fb_ok".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let fallback = Pipeline {
        name: "fb".into(),
        steps: vec![fb_observe],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let main = Pipeline {
        name: "main".into(),
        steps: vec![primary_ok, fail_step],
        on_failure: FailureMode::Fallback(Box::new(fallback)),
        max_retries: 0,
    };

    let agent = simple_agent(&main);
    let mut runner = PipelineRunner::new();
    let result = runner.run(&main, &agent, json!({})).await;
    assert!(result.is_ok(), "fallback should rescue the pipeline");

    let snapshot = observed_keys.lock().unwrap().expect("fallback ran");
    assert_eq!(
        snapshot, 0,
        "fallback step_results must be clean, got {snapshot} keys"
    );
}

// â”€â”€â”€ Test 7: Both primary and fallback fail â†’ Err propagated â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_fallback_also_fails_propagates_error() {
    let primary_fail = custom_step("primary_fail", |_| {
        Err(StepError::ActionFailed { reason: "p".into() })
    });
    let fb_also_fails = custom_step("fb_also_fails", |_| {
        Err(StepError::ActionFailed {
            reason: "fb".into(),
        })
    });

    let fallback = Pipeline {
        name: "fb".into(),
        steps: vec![fb_also_fails],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let main = Pipeline {
        name: "main".into(),
        steps: vec![primary_fail],
        on_failure: FailureMode::Fallback(Box::new(fallback)),
        max_retries: 0,
    };

    let agent = simple_agent(&main);
    let mut runner = PipelineRunner::new();
    let result = runner.run(&main, &agent, json!({})).await;
    assert!(result.is_err(), "fallback failure must propagate as Err");
}

// â”€â”€â”€ Test 8: Skip failure mode â†’ success=false with failed/passed recorded â”€â”€â”€â”€

#[tokio::test]
async fn test_skip_on_failure_sets_success_false() {
    let a = custom_step("a", |_| Ok(StepOutput::new("A".into())));
    let b = custom_step("b", |_| {
        Err(StepError::ActionFailed {
            reason: "boom".into(),
        })
    });
    let c = custom_step("c", |_| Ok(StepOutput::new("C".into())));

    let pipeline = Pipeline {
        name: "skip_test".into(),
        steps: vec![a, b, c],
        on_failure: FailureMode::Skip,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(
        !result.success,
        "success must be false when any step failed"
    );
    assert_eq!(result.steps_passed, vec!["a".to_string(), "c".to_string()]);
    assert_eq!(result.steps_failed, vec!["b".to_string()]);
    assert!(result.step_results["b"].error.is_some());
    assert!(!result.step_results["b"].verdict_passed);
}

// â”€â”€â”€ Test 9: InjectionProtection::Strict passes on clean output â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_injection_protection_strict_passes_on_clean_output() {
    let clean = AgentStep {
        name: "clean_step".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new(
                "Hello world, this is a completely normal response.".into(),
            ))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = abort_pipeline("inj_clean", vec![clean]);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert_eq!(result.steps_passed, vec!["clean_step".to_string()]);
    assert!(result.steps_failed.is_empty());
    assert!(!result
        .audit_log
        .entries()
        .iter()
        .any(|e| matches!(e.event, AuditEvent::InjectionDetected { .. })));
}

// â”€â”€â”€ Test 10: step_results map is fully populated after 4-step pipeline â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_step_results_map_is_complete_and_keyed_by_name() {
    let names = ["alpha", "beta", "gamma", "delta"];
    let steps: Vec<AgentStep> = names
        .iter()
        .map(|&n| custom_step(n, move |_ctx| Ok(StepOutput::new(n.to_string()))))
        .collect();

    let pipeline = abort_pipeline("map_check", steps);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert_eq!(result.step_results.len(), 4);
    for name in &names {
        let sr = result.step_results.get(*name).expect("present");
        assert_eq!(sr.step_name, *name);
        assert!(sr.verdict_passed);
        assert_eq!(sr.output.raw, *name);
        assert!(sr.error.is_none());
    }
}

// â”€â”€â”€ Test 11: Full success pipeline has correct audit events â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_full_success_pipeline_audit_contains_started_and_completed() {
    let s1 = AgentStep {
        name: "s1".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| Ok(StepOutput::new("nonempty".into())))),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::None),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    let s2 = AgentStep {
        name: "s2".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| Ok(StepOutput::new(r#"{"k":1}"#.into())))),
        guard_out: Guard::AllOf(vec![Guard::NonEmptyOutput, Guard::ValidJson]),
        verdict: Verdict::Automated(Guard::None),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = abort_pipeline("happy", vec![s1, s2]);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 2);
    assert!(result.steps_failed.is_empty());

    let entries = result.audit_log.entries();
    assert!(entries
        .iter()
        .any(|e| matches!(e.event, AuditEvent::PipelineStarted)));
    assert!(entries.iter().any(|e| matches!(
        e.event,
        AuditEvent::PipelineCompleted {
            steps_failed: 0,
            ..
        }
    )));
}

// â”€â”€â”€ Test 12: ctx.request mirrors the input passed to run() â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_step_reads_original_request_from_ctx() {
    let s1 = custom_step("step1", |ctx| {
        let task = ctx
            .request
            .get("task")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prio = ctx
            .request
            .get("priority")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        Ok(StepOutput::new(format!("task={task}|priority={prio}")))
    });
    let s2 = custom_step("step2", |ctx| {
        let prev = ctx
            .step_results
            .get("step1")
            .ok_or_else(|| StepError::ActionFailed {
                reason: "no step1".into(),
            })?
            .output
            .raw
            .clone();
        Ok(StepOutput::new(format!("echo:{prev}")))
    });

    let pipeline = abort_pipeline("request_chain", vec![s1, s2]);
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let input = json!({ "task": "summarize", "priority": "high" });
    let result = runner.run(&pipeline, &agent, input).await.unwrap();

    assert!(result.success);
    assert_eq!(
        result.step_results["step1"].output.raw,
        "task=summarize|priority=high"
    );
    assert_eq!(
        result.step_results["step2"].output.raw,
        "echo:task=summarize|priority=high"
    );
}
