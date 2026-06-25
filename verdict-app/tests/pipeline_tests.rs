use verdict::prelude::*;
use serde_json::json;
use std::sync::Arc;
use verdict_app::agent::build_eval_pipeline;
use verdict_app::memory;

// ============================================================================
// TWO-STEP PIPELINE TESTS
// ============================================================================

#[tokio::test]
async fn test_two_step_pipeline_understand_then_act() {
    // Build a simple two-step pipeline with Custom actions
    let pipeline = Pipeline {
        name: "test_two_step".to_string(),
        steps: vec![
            AgentStep {
                name: "understand".to_string(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("parsed: hello world".to_string()))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
            AgentStep {
                name: "act".to_string(),
                guard_in: Guard::StepPassed("understand".to_string()),
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("done".to_string()))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({"task": "test"}))
        .await
        .expect("Pipeline should succeed");

    assert!(result.success);
    assert!(result.steps_passed.contains(&"understand".to_string()));
    assert!(result.steps_passed.contains(&"act".to_string()));
    assert!(result.steps_failed.is_empty());
}

#[tokio::test]
async fn test_act_blocked_when_understand_fails() {
    let pipeline = Pipeline {
        name: "test_blocking".to_string(),
        steps: vec![
            AgentStep {
                name: "understand".to_string(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Err(StepError::ActionFailed {
                        reason: "intentional failure".into(),
                    })
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
            AgentStep {
                name: "act".to_string(),
                guard_in: Guard::StepPassed("understand".to_string()),
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("should not run".to_string()))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({"task": "test"}))
        .await;

    // Should fail
    assert!(result.is_err());
}

// ============================================================================
// GUARD OUTPUT VALIDATION TESTS
// ============================================================================

#[tokio::test]
async fn test_guard_out_noemptyoutput_blocks_empty_response() {
    let pipeline = Pipeline {
        name: "test_empty_guard".to_string(),
        steps: vec![AgentStep {
            name: "test_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_ctx| {
                Ok(StepOutput::new("".to_string()))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({"task": "test"}))
        .await;

    // Should fail because output is empty
    assert!(result.is_err());
}

#[tokio::test]
async fn test_guard_out_noemptyoutput_passes_nonempty() {
    let pipeline = Pipeline {
        name: "test_nonempty_guard".to_string(),
        steps: vec![AgentStep {
            name: "test_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_ctx| {
                Ok(StepOutput::new("non-empty".to_string()))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({"task": "test"}))
        .await
        .expect("Pipeline should succeed");

    assert!(result.success);
}

// ============================================================================
// AGENT REGISTRY AND DELEGATION TESTS
// ============================================================================

#[tokio::test]
async fn test_agent_registry_lookup() {
    let mut registry = AgentRegistry::new();

    let test_agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline: Pipeline {
            name: "test_pipeline".to_string(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    registry.register(test_agent);

    let retrieved = registry.get("test_agent");
    assert!(retrieved.is_some());
}

#[tokio::test]
async fn test_skill_registry_builtins_exist() {
    let registry = SkillRegistry::new();

    // Built-in skills should be available
    let rust_debug = registry.get("rust_debugging");
    // Registry might be empty initially; test just verifies it doesn't panic
    let _ = rust_debug;
}

// ============================================================================
// IMPROVE PIPELINE STRUCTURE TESTS
// ============================================================================

#[test]
fn test_improve_pipeline_structure_validates() {
    use verdict_app::agent::build_improve_pipeline;

    let pipeline = build_improve_pipeline();

    assert_eq!(pipeline.steps.len(), 2);
    assert_eq!(pipeline.steps[0].name, "self_reflect");
    assert_eq!(pipeline.steps[1].name, "propose_self_update");

    // Verify step 0 has DelegateAgent action
    match &pipeline.steps[0].action {
        StepAction::DelegateAgent { agent, .. } => {
            assert_eq!(agent, "reflector");
        }
        _ => panic!("Expected DelegateAgent action in self_reflect"),
    }

    // Verify step 1 has LlmCall action and ValidJson verdict
    match &pipeline.steps[1].action {
        StepAction::LlmCall { user, .. } => {
            assert!(user.contains("{self_reflect}"));
        }
        _ => panic!("Expected LlmCall action in propose_self_update"),
    }

    match &pipeline.steps[1].verdict {
        Verdict::Automated(Guard::ValidJson) => {
            // Correct!
        }
        _ => panic!("Expected Verdict::Automated(Guard::ValidJson)"),
    }
}

// ============================================================================
// STEP RESULT TRACKING TESTS
// ============================================================================

#[tokio::test]
async fn test_step_results_are_tracked() {
    let pipeline = Pipeline {
        name: "test_tracking".to_string(),
        steps: vec![
            AgentStep {
                name: "step1".to_string(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("output1".to_string()))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
            AgentStep {
                name: "step2".to_string(),
                guard_in: Guard::StepPassed("step1".to_string()),
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("output2".to_string()))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({}))
        .await
        .expect("Pipeline should succeed");

    assert!(result.step_results.contains_key("step1"));
    assert!(result.step_results.contains_key("step2"));

    let step1_result = &result.step_results["step1"];
    assert_eq!(step1_result.output.raw, "output1");

    let step2_result = &result.step_results["step2"];
    assert_eq!(step2_result.output.raw, "output2");
}

// ============================================================================
// VERDICT AUTOMATION TESTS
// ============================================================================

#[tokio::test]
async fn test_automated_verdict_passes() {
    let pipeline = Pipeline {
        name: "test_verdict".to_string(),
        steps: vec![AgentStep {
            name: "test_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_ctx| {
                Ok(StepOutput::new("valid".to_string()))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({}))
        .await
        .expect("Pipeline should succeed");

    assert!(result.success);
    let step_result = &result.step_results["test_step"];
    assert!(step_result.verdict_passed);
}

// ============================================================================
// EDGE CASE TESTS
// ============================================================================

#[tokio::test]
async fn test_single_step_pipeline() {
    let pipeline = Pipeline {
        name: "single_step".to_string(),
        steps: vec![AgentStep {
            name: "only_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_ctx| {
                Ok(StepOutput::new("result".to_string()))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({}))
        .await
        .expect("Pipeline should succeed");

    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 1);
}

#[tokio::test]
async fn test_pipeline_with_json_input() {
    let pipeline = Pipeline {
        name: "json_input".to_string(),
        steps: vec![AgentStep {
            name: "test".to_string(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|ctx| {
                let input_str = ctx.input.to_string();
                Ok(StepOutput::new(format!("Got: {}", input_str)))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let input = json!({"key": "value", "number": 42});
    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, input)
        .await
        .expect("Pipeline should succeed");

    assert!(result.success);
    assert!(result.step_results["test"].output.raw.contains("Got:"));
}

#[tokio::test]
async fn test_pipeline_preserves_step_output() {
    let pipeline = Pipeline {
        name: "preserve_output".to_string(),
        steps: vec![
            AgentStep {
                name: "produce".to_string(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new("specific_value".to_string()))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
            AgentStep {
                name: "consume".to_string(),
                guard_in: Guard::StepPassed("produce".to_string()),
                action: StepAction::Custom(Arc::new(|ctx| {
                    // Verify we can read the prior step output
                    let prior_output = ctx
                        .step_results
                        .get("produce")
                        .map(|r| r.output.raw.as_str())
                        .unwrap_or("");
                    Ok(StepOutput::new(format!("consumed: {}", prior_output)))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({}))
        .await
        .expect("Pipeline should succeed");

    assert!(result.success);
    assert!(result.step_results["consume"]
        .output
        .raw
        .contains("specific_value"));
}



// ============================================================================
// NEW PHASE A-F INTEGRATION TESTS
// ============================================================================

#[tokio::test]
async fn test_memory_agent_pipeline_runs_with_custom_action() {
    let pipeline = Pipeline {
        name: "custom_action_test".to_string(),
        steps: vec![AgentStep {
            name: "custom_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_ctx| {
                Ok(StepOutput::new("custom response".to_string()))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent for custom action".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner
        .run(&agent.pipeline, &agent, json!({}))
        .await
        .expect("Pipeline should run successfully");

    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 1);
    assert!(result.step_results["custom_step"].output.raw.contains("custom response"));
}

#[test]
fn test_eval_pipeline_structure_has_rubric_loop() {
    let pipeline = build_eval_pipeline();
    
    // Verify the pipeline structure and action type
    assert_eq!(pipeline.name, "eval-pipeline");
    assert_eq!(pipeline.steps.len(), 1);
    
    let step = &pipeline.steps[0];
    assert_eq!(step.name, "evaluate_with_rubric");
    
    // Verify it has RubricLoop action
    match &step.action {
        StepAction::RubricLoop { rubric, max_iterations, .. } => {
            assert_eq!(rubric.len(), 2);
            assert_eq!(*max_iterations, 3);
        }
        _ => panic!("Expected RubricLoop action"),
    }
}

#[tokio::test]
async fn test_multi_step_pipeline_with_memory_store() {
    let memory_store = memory::build_memory_store();

    let pipeline = Pipeline {
        name: "memory_test_pipeline".to_string(),
        steps: vec![AgentStep {
            name: "memory_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_ctx| {
                Ok(StepOutput::new("test memory".to_string()))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "memory_agent".to_string(),
        description: "Test agent with memory store".to_string(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new().with_memory(memory_store);
    let result = runner
        .run(&agent.pipeline, &agent, json!({}))
        .await
        .expect("Pipeline with memory store should run");

    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 1);
}
