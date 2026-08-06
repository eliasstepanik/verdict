//! Integration tests: AuditLog, BudgetTracker, DAG pipelines, parallel steps, ContextStore
//!
//! Deterministic tests ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â no LLM required. Verifies audit event ordering/counts,
//! budget counters, DAG execution order, parallel result merging, and checkpoint files.

use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use verdict::prelude::*;
use verdict::ContextStore;

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ helpers ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

fn ok_step(name: &'static str) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| Ok(StepOutput::new("ok".into())))),
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

#[allow(dead_code)]
fn ok_step_with_deps(name: &'static str, deps: Vec<&'static str>) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| Ok(StepOutput::new("ok".into())))),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: deps.into_iter().map(String::from).collect(),
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

fn simple_agent(pipeline: &Pipeline) -> Agent {
    Agent {
        name: "t".into(),
        description: "t".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: vec![],
    }
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 1: 3-step success pipeline has exact audit event counts ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_audit_log_three_step_pipeline_exact_event_counts() {
    let pipeline = Pipeline {
        name: "p3".into(),
        steps: vec![ok_step("a"), ok_step("b"), ok_step("c")],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let result = PipelineRunner::new()
        .run(&pipeline, &agent, json!(null))
        .await
        .unwrap();

    let mut pipeline_started = 0;
    let mut step_started = 0;
    let mut step_completed_pass = 0;
    let mut step_failed = 0;
    let mut pipe_completed: Option<(u32, u32)> = None;
    let mut pipe_failed = 0;

    for e in result.audit_log.entries() {
        match &e.event {
            AuditEvent::PipelineStarted => pipeline_started += 1,
            AuditEvent::StepStarted => step_started += 1,
            AuditEvent::StepCompleted {
                verdict_passed: true,
            } => step_completed_pass += 1,
            AuditEvent::StepFailed { .. } => step_failed += 1,
            AuditEvent::PipelineCompleted {
                steps_passed,
                steps_failed,
            } => {
                pipe_completed = Some((*steps_passed, *steps_failed));
            }
            AuditEvent::PipelineFailed { .. } => pipe_failed += 1,
            _ => {}
        }
    }

    assert_eq!(pipeline_started, 1, "exactly one PipelineStarted");
    assert_eq!(step_started, 3, "exactly three StepStarted");
    assert_eq!(
        step_completed_pass, 3,
        "exactly three StepCompleted(passed)"
    );
    assert_eq!(step_failed, 0, "no StepFailed in success path");
    assert_eq!(pipe_failed, 0, "no PipelineFailed in success path");
    assert_eq!(
        pipe_completed,
        Some((3, 0)),
        "PipelineCompleted with 3 passed, 0 failed"
    );
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 2: StepFailed emitted on action failure, not StepCompleted ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_audit_log_step_failed_emitted_not_step_completed() {
    let good = ok_step("good");
    let bad = AgentStep {
        name: "bad".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| {
            Err(StepError::ActionFailed {
                reason: "boom".into(),
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
        name: "fail_p".into(),
        steps: vec![good, bad],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let _ = runner.run(&pipeline, &agent, json!(null)).await;

    // On Abort, result is Err ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â inspect runner.audit_log directly
    let entries = runner.audit_log.entries();

    let good_completed = entries
        .iter()
        .filter(|e| {
            e.step_name == "good"
                && matches!(
                    e.event,
                    AuditEvent::StepCompleted {
                        verdict_passed: true
                    }
                )
        })
        .count();
    let good_failed = entries
        .iter()
        .filter(|e| e.step_name == "good" && matches!(e.event, AuditEvent::StepFailed { .. }))
        .count();
    let bad_completed = entries
        .iter()
        .filter(|e| e.step_name == "bad" && matches!(e.event, AuditEvent::StepCompleted { .. }))
        .count();
    let bad_failed_with_boom = entries
        .iter()
        .filter(|e| {
            e.step_name == "bad"
                && matches!(&e.event, AuditEvent::StepFailed { error } if error.contains("boom"))
        })
        .count();

    assert_eq!(
        good_completed, 1,
        "good step emits StepCompleted exactly once"
    );
    assert_eq!(good_failed, 0, "good step never fails");
    assert_eq!(bad_completed, 0, "bad step must not emit StepCompleted");
    assert_eq!(
        bad_failed_with_boom, 1,
        "bad step emits StepFailed with reason containing 'boom'"
    );
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 3: ToolCallStarted/Completed pair for each FunctionTool call ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_audit_log_tool_call_started_and_completed_pairs() {
    let echo = FunctionTool::new(
        "local.echo",
        "Echo input",
        json!({ "type": "object", "properties": { "msg": {"type": "string"} } }),
        |args, _ctx| {
            Box::pin(async move {
                let m = args["msg"].as_str().unwrap_or("").to_string();
                Ok(ToolOutput::text(format!("echo:{m}")))
            })
        },
    );
    let mut tr = ToolRegistry::new();
    tr.register(echo);

    fn call(name: &str, msg: &str) -> AgentStep {
        AgentStep {
            name: name.into(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "local.echo".into(),
                args: json!({"msg": msg}),
            },
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Allow(vec!["local.echo".into()]),
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }
    }

    let pipeline = Pipeline {
        name: "tools".into(),
        steps: vec![call("s1", "alpha"), call("s2", "beta")],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = Agent {
        name: "t".into(),
        description: "t".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Allow(vec!["local.echo".into()]),
        skills: SkillSet::default(),
        policy: AgentPolicy {
            allowed_tools: ToolSet::Allow(vec!["local.echo".into()]),
            ..Default::default()
        },
        scorers: vec![],
    };

    let result = PipelineRunner::with_tool_registry(Arc::new(tr))
        .run(&pipeline, &agent, json!(null))
        .await
        .unwrap();

    let entries = result.audit_log.entries();
    let started_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            matches!(&e.event,
            AuditEvent::ToolCallStarted { tool, .. } if tool == "local.echo")
        })
        .map(|(i, _)| i)
        .collect();
    let completed_pairs: Vec<(usize, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| match &e.event {
            AuditEvent::ToolCallCompleted { tool, output_bytes } if tool == "local.echo" => {
                Some((i, *output_bytes))
            }
            _ => None,
        })
        .collect();
    let failed = entries
        .iter()
        .filter(|e| matches!(e.event, AuditEvent::ToolCallFailed { .. }))
        .count();

    assert_eq!(
        started_indices.len(),
        2,
        "two ToolCallStarted for local.echo"
    );
    assert_eq!(
        completed_pairs.len(),
        2,
        "two ToolCallCompleted for local.echo"
    );
    assert_eq!(failed, 0, "no ToolCallFailed");

    for (si, (ci, bytes)) in started_indices.iter().zip(completed_pairs.iter()) {
        assert!(
            si < ci,
            "ToolCallStarted index {si} must precede ToolCallCompleted index {ci}"
        );
        assert!(*bytes > 0, "output_bytes must be > 0 for non-empty output");
    }
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 4: GuardFailed event has non-empty reason in payload ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_audit_log_guard_failed_has_non_empty_reason() {
    let bad_json_step = AgentStep {
        name: "produce_garbage".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| {
            Ok(StepOutput::new("this is not json {[".into()))
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
    let pipeline = Pipeline {
        name: "guards".into(),
        steps: vec![bad_json_step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();
    let outcome = runner.run(&pipeline, &agent, json!(null)).await;

    // May return Err (Abort) ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â inspect runner.audit_log
    let entries = runner.audit_log.entries();
    let guard_failed_with_reason = entries
        .iter()
        .filter(|e| {
            matches!(&e.event, AuditEvent::GuardFailed { guard, reason }
            if guard.contains("ValidJson") && !reason.is_empty())
        })
        .count();
    let step_completed_pass = entries
        .iter()
        .filter(|e| {
            e.step_name == "produce_garbage"
                && matches!(
                    e.event,
                    AuditEvent::StepCompleted {
                        verdict_passed: true
                    }
                )
        })
        .count();

    assert_eq!(
        guard_failed_with_reason, 1,
        "exactly one GuardFailed(ValidJson) with non-empty reason"
    );
    assert_eq!(
        step_completed_pass, 0,
        "no successful StepCompleted for failed guard step"
    );

    match outcome {
        Ok(r) => assert!(!r.success, "result.success must be false"),
        Err(PipelineError::GuardFailed {
            phase: GuardPhase::Out,
            ..
        }) => {}
        Err(e) => panic!("unexpected error: {e:?}"),
    }
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 5: GuardPassed events emitted for each passing guard ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_audit_log_guard_passed_per_passing_guard() {
    let step = AgentStep {
        name: "json_step".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_| Ok(StepOutput::new(r#"{"k":1}"#.into())))),
        guard_out: Guard::ValidJson,
        verdict: Verdict::Automated(Guard::None),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    let pipeline = Pipeline {
        name: "gp".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let r = PipelineRunner::new()
        .run(&pipeline, &agent, json!({"task":"hi"}))
        .await
        .unwrap();

    let passed_valid_json = r.audit_log.entries().iter().filter(|e|
        matches!(&e.event, AuditEvent::GuardPassed { guard } if guard.contains("ValidJson"))
    ).count();
    let any_failed = r
        .audit_log
        .entries()
        .iter()
        .any(|e| matches!(e.event, AuditEvent::GuardFailed { .. }));
    let completed_pass = r
        .audit_log
        .entries()
        .iter()
        .filter(|e| {
            matches!(
                e.event,
                AuditEvent::StepCompleted {
                    verdict_passed: true
                }
            )
        })
        .count();

    assert!(
        passed_valid_json >= 1,
        "at least one GuardPassed(ValidJson)"
    );
    assert!(!any_failed, "no GuardFailed events");
    assert_eq!(completed_pass, 1, "one StepCompleted(passed)");
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 6: Diamond DAG ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â A first, D last, B and C in between ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_dag_diamond_execution_order() {
    let counter = Arc::new(AtomicUsize::new(0));
    let order: Arc<Mutex<HashMap<String, usize>>> = Arc::new(Mutex::new(HashMap::new()));

    let mk = |name: &'static str,
              deps: Vec<&'static str>,
              c: Arc<AtomicUsize>,
              o: Arc<Mutex<HashMap<String, usize>>>|
     -> AgentStep {
        AgentStep {
            name: name.into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| {
                let idx = c.fetch_add(1, Ordering::SeqCst);
                o.lock().unwrap().insert(name.to_string(), idx);
                Ok(StepOutput::new("ok".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: deps.into_iter().map(String::from).collect(),
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }
    };

    let a = mk("A", vec![], counter.clone(), order.clone());
    let b = mk("B", vec!["A"], counter.clone(), order.clone());
    let c = mk("C", vec!["A"], counter.clone(), order.clone());
    let d = mk("D", vec!["B", "C"], counter.clone(), order.clone());

    let pipeline = Pipeline {
        name: "diamond".into(),
        steps: vec![a, b, c, d],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let r = PipelineRunner::new()
        .run(&pipeline, &agent, json!(null))
        .await
        .unwrap();

    assert!(r.success);
    assert_eq!(r.steps_passed.len(), 4);

    let o = order.lock().unwrap();
    let pa = o["A"];
    let pb = o["B"];
    let pc = o["C"];
    let pd = o["D"];

    assert_eq!(pa, 0, "A must run first");
    assert_eq!(pd, 3, "D must run last");
    assert!(pb > pa && pc > pa, "B and C must run after A");
    assert!(pd > pb && pd > pc, "D must run after B and C");
    assert!(
        (pb == 1 && pc == 2) || (pb == 2 && pc == 1),
        "B and C occupy positions 1 and 2 (in either order)"
    );
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 7: Parallel steps both complete and results are merged ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_parallel_steps_results_merged_in_step_results() {
    fn par(name: &'static str, payload: &'static str) -> AgentStep {
        AgentStep {
            name: name.into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| Ok(StepOutput::new(payload.into())))),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: true,
            input_processors: vec![],
            output_processors: vec![],
        }
    }

    let pipeline = Pipeline {
        name: "par".into(),
        steps: vec![par("p1", "result-1"), par("p2", "result-2")],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let r = PipelineRunner::new()
        .run(&pipeline, &agent, json!(null))
        .await
        .unwrap();

    assert!(r.success);
    assert_eq!(r.steps_passed.len(), 2);
    assert!(
        r.step_results.contains_key("p1"),
        "p1 must be in step_results"
    );
    assert!(
        r.step_results.contains_key("p2"),
        "p2 must be in step_results"
    );
    assert_eq!(r.step_results["p1"].output.raw, "result-1");
    assert_eq!(r.step_results["p2"].output.raw, "result-2");
    assert!(r.step_results["p1"].verdict_passed);
    assert!(r.step_results["p2"].verdict_passed);
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 8: with_context_store writes checkpoint files for each step ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_context_store_checkpoint_files_written_per_step() {
    let tmp = std::env::temp_dir().join(format!("verdict_ctxstore_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let pipeline = Pipeline {
        name: "ckpt".into(),
        steps: vec![ok_step("a"), ok_step("b"), ok_step("c")],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new().with_context_store(tmp.clone());
    let r = runner.run(&pipeline, &agent, json!(null)).await.unwrap();
    assert!(r.success);

    // Verify checkpoint files exist for each step
    for step_name in ["a", "b", "c"] {
        let p = tmp.join(format!("ckpt_{step_name}.json"));
        assert!(p.exists(), "checkpoint must exist: {p:?}");
        let bytes = std::fs::read(&p).unwrap();
        assert!(!bytes.is_empty(), "checkpoint must be non-empty");
        // Must be valid JSON deserializable as SerializableStepContext
        let ctx: verdict::context::SerializableStepContext = serde_json::from_slice(&bytes)
            .expect("checkpoint must deserialize as SerializableStepContext");
        assert_eq!(ctx.pipeline_name, "ckpt");
        assert_eq!(ctx.step_name, step_name);
    }

    // list_snapshots should return exactly 3
    let store = ContextStore::new(tmp.clone());
    let snaps = store.list_snapshots("ckpt").await.unwrap();
    assert_eq!(snaps.len(), 3, "exactly 3 snapshots listed");

    let _ = std::fs::remove_dir_all(&tmp);
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 9: ContextStore round-trip preserves all serializable fields ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_context_store_round_trip_preserves_all_fields() {
    let tmp = std::env::temp_dir().join(format!("verdict_rt_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    let fs_policy = FilesystemPolicy {
        workspace_root: std::env::current_dir().unwrap(),
        ..Default::default()
    };
    let mut ctx = StepContext::new(
        "agent_x".into(),
        "p_round_trip".into(),
        "step1".into(),
        json!({"task": "go"}),
        fs_policy,
    );
    ctx.input = json!({"k": "v"});
    ctx.output = Some(StepOutput::new("hello".into()));
    ctx.delegation_depth = 2;
    ctx.parent_agent = Some("parent".into());
    ctx.active_skills = vec!["sk".into()];
    ctx.budget.llm_calls_used = 5;
    ctx.budget.tool_calls_used = 3;
    ctx.trace.entries.push(verdict::context::TraceEntry {
        step_name: "step1".into(),
        status: "ok".into(),
        timestamp: chrono::Utc::now(),
    });
    ctx.conversation_history.push(ChatRole::User, "hi".into());
    ctx.conversation_history
        .push(ChatRole::Assistant, "hello".into());

    let store = ContextStore::new(tmp.clone());
    store.save(&ctx).await.expect("save must succeed");
    let loaded = store
        .load("p_round_trip", "step1")
        .await
        .expect("load must succeed");

    assert_eq!(loaded.agent_name, "agent_x");
    assert_eq!(loaded.pipeline_name, "p_round_trip");
    assert_eq!(loaded.step_name, "step1");
    assert_eq!(loaded.request, json!({"task": "go"}));
    assert_eq!(loaded.input, json!({"k": "v"}));
    assert_eq!(loaded.output.as_ref().unwrap().raw, "hello");
    assert_eq!(loaded.delegation_depth, 2);
    assert_eq!(loaded.parent_agent.as_deref(), Some("parent"));
    assert_eq!(loaded.active_skills, vec!["sk".to_string()]);
    assert_eq!(loaded.budget.llm_calls_used, 5);
    assert_eq!(loaded.budget.tool_calls_used, 3);
    assert_eq!(loaded.trace.entries.len(), 1);
    assert_eq!(loaded.conversation_history.messages.len(), 2);

    let _ = std::fs::remove_dir_all(&tmp);
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 10: tool_calls_used increments per FunctionTool call ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_budget_tool_calls_used_increments_per_tool_call() {
    let noop = FunctionTool::new(
        "local.noop",
        "noop",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Ok(ToolOutput::text("x".to_string())) }),
    );
    let mut tr = ToolRegistry::new();
    tr.register(noop);

    let observed: Arc<Mutex<Option<(u32, u32)>>> = Arc::new(Mutex::new(None));
    let obs = Arc::clone(&observed);

    fn tc(name: &str) -> AgentStep {
        AgentStep {
            name: name.into(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "local.noop".into(),
                args: json!({}),
            },
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Allow(vec!["local.noop".into()]),
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }
    }

    let introspect = AgentStep {
        name: "introspect".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |ctx| {
            *obs.lock().unwrap() = Some((ctx.budget.llm_calls_used, ctx.budget.tool_calls_used));
            Ok(StepOutput::new("done".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["t1".into(), "t2".into(), "t3".into()],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "bud".into(),
        steps: vec![tc("t1"), tc("t2"), tc("t3"), introspect],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = Agent {
        name: "t".into(),
        description: "t".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Allow(vec!["local.noop".into()]),
        skills: SkillSet::default(),
        policy: AgentPolicy {
            allowed_tools: ToolSet::Allow(vec!["local.noop".into()]),
            ..Default::default()
        },
        scorers: vec![],
    };

    let r = PipelineRunner::with_tool_registry(Arc::new(tr))
        .run(&pipeline, &agent, json!(null))
        .await
        .unwrap();
    assert_eq!(r.steps_passed.len(), 4);

    let (llm, tool) = observed.lock().unwrap().expect("introspect step ran");
    assert_eq!(llm, 0, "no LLM calls were made");
    assert_eq!(
        tool, 3,
        "tool_calls_used must equal the number of ToolCall steps (3)"
    );
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 11: Missing DAG dependency is rejected ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_dag_missing_dependency_is_rejected() {
    let a = ok_step("a");
    let mut b = ok_step("b");
    b.dependencies = vec!["nonexistent".into()];

    let pipeline = Pipeline {
        name: "missing_dep".into(),
        steps: vec![a, b],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = simple_agent(&pipeline);
    let mut runner = PipelineRunner::new();

    // Try sort-level rejection first
    let sort_result = runner.topological_sort(&pipeline);
    if let Err(e) = &sort_result {
        let msg = format!("{e:?}");
        assert!(
            msg.contains("nonexistent"),
            "sort error must reference missing dep: {msg}"
        );
        return; // test passed at sort level
    }

    // Fallback: expect runtime rejection
    let outcome = runner.run(&pipeline, &agent, json!(null)).await;
    let err = outcome.expect_err("missing dep must cause pipeline failure");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("nonexistent") || msg.contains("missing") || msg.contains("not found"),
        "runtime error must reference missing dep: {msg}"
    );
}

// ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ Test 12: Audit event ordering within a single step ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬

#[tokio::test]
async fn test_audit_log_event_ordering_within_step() {
    let ping = FunctionTool::new(
        "local.ping",
        "ping",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Ok(ToolOutput::text("pong".to_string())) }),
    );
    let mut tr = ToolRegistry::new();
    tr.register(ping);

    let step = AgentStep {
        name: "s1".into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall {
            tool: "local.ping".into(),
            args: json!({}),
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        tools: ToolSet::Allow(vec!["local.ping".into()]),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };
    let pipeline = Pipeline {
        name: "ord".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = Agent {
        name: "t".into(),
        description: "t".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Allow(vec!["local.ping".into()]),
        skills: SkillSet::default(),
        policy: AgentPolicy {
            allowed_tools: ToolSet::Allow(vec!["local.ping".into()]),
            ..Default::default()
        },
        scorers: vec![],
    };

    let r = PipelineRunner::with_tool_registry(Arc::new(tr))
        .run(&pipeline, &agent, json!(null))
        .await
        .unwrap();

    let slice: Vec<&AuditEntry> = r
        .audit_log
        .entries()
        .iter()
        .filter(|e| e.step_name == "s1")
        .collect();

    let idx_step_started = slice
        .iter()
        .position(|e| matches!(e.event, AuditEvent::StepStarted))
        .expect("StepStarted present");
    let idx_step_completed = slice
        .iter()
        .position(|e| matches!(e.event, AuditEvent::StepCompleted { .. }))
        .expect("StepCompleted present");
    let idx_tool_started = slice.iter().position(|e|
        matches!(&e.event, AuditEvent::ToolCallStarted { tool, .. } if tool == "local.ping")
    ).expect("ToolCallStarted present");
    let idx_tool_completed = slice.iter().position(|e|
        matches!(&e.event, AuditEvent::ToolCallCompleted { tool, .. } if tool == "local.ping")
    ).expect("ToolCallCompleted present");

    assert_eq!(
        idx_step_started, 0,
        "StepStarted must be first event for the step"
    );
    assert!(
        idx_tool_started < idx_tool_completed,
        "ToolCallStarted must precede ToolCallCompleted"
    );
    assert!(
        idx_tool_completed < idx_step_completed,
        "ToolCallCompleted must precede StepCompleted"
    );
    assert!(
        idx_step_started < idx_tool_started,
        "StepStarted must precede ToolCallStarted"
    );
}
