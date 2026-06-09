use std::sync::{Arc, Mutex};
use verdict::prelude::*;
use serde_json::Value;

#[tokio::test]
async fn test_guard_none_always_passes() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );
    let result = GuardEngine::evaluate(&Guard::None, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_validjson_accepts_valid_json() {
    let mut ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );
    ctx.output = Some(StepOutput::new(r#"{"key": "value"}"#.into()));

    let result = GuardEngine::evaluate(&Guard::ValidJson, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_validjson_rejects_invalid_json() {
    let mut ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );
    ctx.output = Some(StepOutput::new("not json".into()));

    let result = GuardEngine::evaluate(&Guard::ValidJson, &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_guard_fileexists() {
    let mut ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );
    // Use Cargo.toml which should exist
    ctx.filesystem_policy.workspace_root = std::env::current_dir().unwrap();

    let result = GuardEngine::evaluate(&Guard::FileExists("Cargo.toml".into()), &ctx).await;
    // Should pass if running in the project root
    assert!(result.is_ok() || result.is_err()); // Accept either result depending on test location
}

#[tokio::test]
#[ignore] // Can conflict with other cargo operations
async fn test_guard_compiles_ok() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );

    // This test runs cargo check on the verdict project itself
    let result = GuardEngine::evaluate(&Guard::Compiles, &ctx).await;
    assert!(
        result.is_ok(),
        "Verdict project should compile: {:?}",
        result
    );
}

#[tokio::test]
#[ignore] // Conflicts with test runner (tries to run cargo test while cargo test is already running)
async fn test_guard_tests_pass_ok() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );

    // This test runs cargo test on the verdict project itself
    let result = GuardEngine::evaluate(&Guard::TestsPass, &ctx).await;
    assert!(
        result.is_ok(),
        "Verdict tests should pass: {:?}",
        result
    );
}

#[tokio::test]
async fn test_guard_allof_all_pass() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );

    let guards = vec![Guard::None, Guard::None];
    let result = GuardEngine::evaluate(&Guard::AllOf(guards), &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_allof_one_fails() {
    let mut ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );
    ctx.output = Some(StepOutput::new("not json".into()));

    let guards = vec![Guard::None, Guard::ValidJson];
    let result = GuardEngine::evaluate(&Guard::AllOf(guards), &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_guard_anyof_one_passes() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );

    let guards = vec![Guard::None, Guard::None];
    let result = GuardEngine::evaluate(&Guard::AnyOf(guards), &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_verdict_none_passes() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );

    let result = VerdictEngine::evaluate(&Verdict::None, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_verdict_automated_passes_when_guard_passes() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );

    let result = VerdictEngine::evaluate(&Verdict::Automated(Guard::None), &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_verdict_user_approval_requires_decision() {
    let ctx = StepContext::new(
        "test".into(),
        "test".into(),
        "test".into(),
        Value::Null,
        Default::default(),
    );

    let result = VerdictEngine::evaluate(
        &Verdict::UserApproval {
            prompt: "Test?",
            show_diff: false,
        },
        &ctx,
    )
    .await;
    assert!(matches!(
        result,
        Err(VerdictError::UserApprovalRequired { .. })
    ));
}

#[tokio::test]
async fn test_pipeline_sequential_3steps() {
    let counter = Arc::new(Mutex::new(0));

    let step1 = {
        let counter = Arc::clone(&counter);
        AgentStep {
            name: "step1".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_ctx| {
                let mut c = counter.lock().unwrap();
                *c += 1;
                assert_eq!(*c, 1);
                Ok(StepOutput::new("step1 output".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }
    };

    let step2 = {
        let counter = Arc::clone(&counter);
        AgentStep {
            name: "step2".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_ctx| {
                let mut c = counter.lock().unwrap();
                *c += 1;
                assert_eq!(*c, 2);
                Ok(StepOutput::new("step2 output".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }
    };

    let step3 = {
        let counter = Arc::clone(&counter);
        AgentStep {
            name: "step3".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_ctx| {
                let mut c = counter.lock().unwrap();
                *c += 1;
                assert_eq!(*c, 3);
                Ok(StepOutput::new("step3 output".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }
    };

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step1, step2, step3],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&pipeline, &agent, Value::Null)
        .await
        .expect("pipeline should succeed");

    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 3);
    assert_eq!(result.steps_failed.len(), 0);
    assert_eq!(*counter.lock().unwrap(), 3);
}

#[tokio::test]
async fn test_pipeline_abort_on_failure() {
    let step1 = AgentStep {
        name: "step1".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("step1".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let step2 = AgentStep {
        name: "step2".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            Err(StepError::ActionFailed {
                reason: "intentional".into(),
            })
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let step3 = AgentStep {
        name: "step3".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            panic!("step3 should not run");
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![step1, step2, step3],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, Value::Null).await;

    assert!(result.is_err(), "Pipeline should fail on abort");
}

#[tokio::test]
async fn test_pipeline_skip_on_failure() {
    let counter = Arc::new(Mutex::new(Vec::<String>::new()));

    let step1 = {
        let counter = Arc::clone(&counter);
        AgentStep {
            name: "step1".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_ctx| {
                counter.lock().unwrap().push("step1".into());
                Ok(StepOutput::new("step1".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }
    };

    let step2 = {
        let counter = Arc::clone(&counter);
        AgentStep {
            name: "step2".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_ctx| {
                counter.lock().unwrap().push("step2".into());
                Err(StepError::ActionFailed {
                    reason: "intentional".into(),
                })
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }
    };

    let step3 = {
        let counter = Arc::clone(&counter);
        AgentStep {
            name: "step3".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_ctx| {
                counter.lock().unwrap().push("step3".into());
                Ok(StepOutput::new("step3".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }
    };

    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![step1, step2, step3],
        on_failure: FailureMode::Skip,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&pipeline, &agent, Value::Null)
        .await
        .expect("pipeline should complete");

    let execution_order = counter.lock().unwrap().clone();
    assert_eq!(execution_order, vec!["step1", "step2", "step3"]);
    assert_eq!(result.steps_passed.len(), 2);
    assert_eq!(result.steps_failed.len(), 1);
}

#[tokio::test]
async fn test_pipeline_retry_then_fail() {
    let attempt_counter = Arc::new(Mutex::new(0));

    let step1 = {
        let counter = Arc::clone(&attempt_counter);
        AgentStep {
            name: "step1".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_ctx| {
                let mut c = counter.lock().unwrap();
                *c += 1;
                if *c < 3 {
                    Err(StepError::ActionFailed {
                        reason: "retry".into(),
                    })
                } else {
                    // Third attempt succeeds
                    Ok(StepOutput::new("success".into()))
                }
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }
    };

    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![step1],
        on_failure: FailureMode::Retry,
        max_retries: 2,
    };

    let agent = Agent {
        name: "test".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&pipeline, &agent, Value::Null)
        .await
        .expect("pipeline should succeed after retries");

    assert!(result.success);
    assert_eq!(*attempt_counter.lock().unwrap(), 3);
}

#[tokio::test]
async fn test_pipeline_sub_pipeline() {
    let inner_pipeline = Pipeline {
        name: "inner".into(),
        steps: vec![
            AgentStep {
                name: "inner_step1".into(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("inner1".into()))
                })),
                guard_out: Guard::None,
                verdict: Verdict::None,
                tools: ToolSet::Full,
                injection_protection: InjectionProtection::None,
                output_schema: None,
            },
            AgentStep {
                name: "inner_step2".into(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("inner2".into()))
                })),
                guard_out: Guard::None,
                verdict: Verdict::None,
                tools: ToolSet::Full,
                injection_protection: InjectionProtection::None,
                output_schema: None,
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let outer_pipeline = Pipeline {
        name: "outer".into(),
        steps: vec![AgentStep {
            name: "outer_step".into(),
            guard_in: Guard::None,
            action: StepAction::SubPipeline(Box::new(inner_pipeline)),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test".into(),
        description: "test".into(),
        pipeline: outer_pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&outer_pipeline, &agent, Value::Null)
        .await
        .expect("pipeline should succeed");

    assert!(result.success);
}

#[tokio::test]
async fn test_loop_until_passes() {
    let iteration_counter = Arc::new(Mutex::new(0));
    let loop_counter = Arc::new(Mutex::new(0));

    let body = {
        let counter = Arc::clone(&loop_counter);
        StepAction::Custom(Arc::new(move |_ctx| {
            let mut c = counter.lock().unwrap();
            *c += 1;
            Ok(StepOutput::new(format!("iteration {}", c)))
        }))
    };

    let condition = {
        let counter = Arc::clone(&iteration_counter);
        Guard::Custom(Arc::new(move |_ctx| {
            let mut c = counter.lock().unwrap();
            *c += 1;
            if *c >= 3 {
                Ok(())
            } else {
                Err(GuardError::Failed {
                    guard: "loop_condition".into(),
                    reason: "not yet".into(),
                })
            }
        }))
    };

    let step = AgentStep {
        name: "loop_step".into(),
        guard_in: Guard::None,
        action: StepAction::LoopUntil {
            body: Box::new(body),
            condition,
            max_iterations: 10,
            on_iteration_failure: IterationFailureMode::Abort,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&pipeline, &agent, Value::Null)
        .await
        .expect("pipeline should succeed");

    assert!(result.success);
    assert_eq!(*iteration_counter.lock().unwrap(), 3);
}

#[tokio::test]
async fn test_loop_until_max_iterations() {
    let iteration_counter = Arc::new(Mutex::new(0));

    let body = StepAction::Custom(Arc::new(|_ctx| {
        Ok(StepOutput::new("iteration".into()))
    }));

    let condition = {
        let counter = Arc::clone(&iteration_counter);
        Guard::Custom(Arc::new(move |_ctx| {
            let mut c = counter.lock().unwrap();
            *c += 1;
            // Never pass the condition
            Err(GuardError::Failed {
                guard: "loop_condition".into(),
                reason: "always fail".into(),
            })
        }))
    };

    let step = AgentStep {
        name: "loop_step".into(),
        guard_in: Guard::None,
        action: StepAction::LoopUntil {
            body: Box::new(body),
            condition,
            max_iterations: 3,
            on_iteration_failure: IterationFailureMode::Abort,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&pipeline, &agent, Value::Null)
        .await
        .expect("pipeline should complete");

    assert!(result.success);
    // Should have tried 3 times and then exited
    assert_eq!(*iteration_counter.lock().unwrap(), 3);
}

#[tokio::test]
async fn test_audit_log_populated() {
    let step1 = AgentStep {
        name: "step1".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("output".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![step1],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test".into(),
        description: "test".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: Default::default(),
        policy: Default::default(),
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&pipeline, &agent, Value::Null)
        .await
        .expect("pipeline should succeed");

    // Check that audit log has entries
    assert!(!result.audit_log.entries().is_empty());
    let entries = result.audit_log.entries();
    // Should have: PipelineStarted, StepStarted, GuardPassed(in), GuardPassed(out), VerdictPassed, StepCompleted, PipelineCompleted
    assert!(entries.len() >= 5);
}

#[tokio::test]
async fn test_audit_log_to_json() {
    let mut log = AuditLog::new();
    log.append(AuditEntry {
        timestamp: chrono::Utc::now(),
        pipeline_name: "test".into(),
        step_name: "step1".into(),
        event: AuditEvent::StepStarted,
    });
    log.append(AuditEntry {
        timestamp: chrono::Utc::now(),
        pipeline_name: "test".into(),
        step_name: "step1".into(),
        event: AuditEvent::GuardPassed {
            guard: "None".into(),
        },
    });

    let json = log.to_json().expect("should convert to JSON");
    assert!(json.contains("StepStarted"));
    assert!(json.contains("GuardPassed"));

    // Verify it's valid JSON
    let _: serde_json::Value = serde_json::from_str(&json).expect("should parse as JSON");
}

#[tokio::test]
async fn test_toolset_contains_full() {
    let toolset = ToolSet::Full;
    assert!(toolset.contains("any_tool"));
    assert!(toolset.contains("fs.read"));
}

#[tokio::test]
async fn test_toolset_contains_allow() {
    let toolset = ToolSet::Allow(vec!["fs.read".into(), "fs.write".into()]);
    assert!(toolset.contains("fs.read"));
    assert!(toolset.contains("fs.write"));
    assert!(!toolset.contains("shell.execute"));
}

#[tokio::test]
async fn test_toolset_contains_deny() {
    let toolset = ToolSet::Deny(vec!["shell.execute".into()]);
    assert!(toolset.contains("fs.read"));
    assert!(!toolset.contains("shell.execute"));
}

#[tokio::test]
async fn test_toolset_intersection() {
    let allow_read = ToolSet::Allow(vec!["fs.read".into()]);
    let allow_all = ToolSet::Full;
    let intersection = ToolSet::Intersection(Box::new(allow_read), Box::new(allow_all));

    assert!(intersection.contains("fs.read"));
    assert!(!intersection.contains("fs.write")); // Only in allow_all, not in allow_read
}

#[tokio::test]
async fn test_toolset_union() {
    let allow_read = ToolSet::Allow(vec!["fs.read".into()]);
    let allow_write = ToolSet::Allow(vec!["fs.write".into()]);
    let union = ToolSet::Union(Box::new(allow_read), Box::new(allow_write));

    assert!(union.contains("fs.read"));
    assert!(union.contains("fs.write"));
    assert!(!union.contains("shell.execute"));
}
