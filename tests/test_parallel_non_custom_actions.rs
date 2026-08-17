use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

/// Test that parallel steps now support non-blocking-Custom actions (e.g., SubPipeline)
#[tokio::test]
async fn test_parallel_steps_with_subpipeline_action() {
    // Create parallel steps with SubPipeline actions (non-Custom variant)
    let inner_pipeline = Pipeline {
        name: "inner_pipeline".to_string(),
        steps: vec![AgentStep {
            name: "inner_step".to_string(),
            action: StepAction::Custom(Arc::new(|_ctx| {
                Ok(StepOutput::new("inner executed".to_string()))
            })),
            guard_in: Guard::None,
            guard_out: Guard::None,
            verdict: Verdict::None,
            parallel: false,
            dependencies: Vec::new(),
            injection_protection: InjectionProtection::None,
            output_schema: None,
            tools: ToolSet::None,
            input_processors: vec![],
            output_processors: vec![],
        }],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let step1 = AgentStep {
        name: "parallel_sub1".to_string(),
        action: StepAction::SubPipeline(Box::new(inner_pipeline.clone())),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
        input_processors: vec![],
        output_processors: vec![],
    };

    let step2 = AgentStep {
        name: "parallel_sub2".to_string(),
        action: StepAction::SubPipeline(Box::new(inner_pipeline.clone())),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "parallel_subpipeline_test".to_string(),
        steps: vec![step1, step2],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();

    // This should now succeed with SubPipeline actions in parallel (was failing before refactor)
    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(
        result.is_ok(),
        "Parallel pipeline with SubPipeline actions should succeed. Error: {:?}",
        result.err()
    );

    let pr = result.unwrap();
    assert!(
        pr.step_results.contains_key("parallel_sub1"),
        "parallel_sub1 result should be present"
    );
    assert!(
        pr.step_results.contains_key("parallel_sub2"),
        "parallel_sub2 result should be present"
    );

    // Both steps should have executed successfully
    let sr1 = &pr.step_results["parallel_sub1"];
    let sr2 = &pr.step_results["parallel_sub2"];
    assert!(sr1.verdict_passed, "parallel_sub1 should have passed");
    assert!(sr2.verdict_passed, "parallel_sub2 should have passed");
}

/// Test that guard_in is now evaluated for parallel steps
#[tokio::test]
async fn test_parallel_steps_guard_in_evaluated() {
    use std::sync::atomic::{AtomicBool, Ordering};

    // Create a simple guard that will fail
    let guard_evaluated = Arc::new(AtomicBool::new(false));
    let guard_eval_clone = guard_evaluated.clone();

    // Create parallel steps with a Custom action and a guard_in that should be evaluated
    let step1 = AgentStep {
        name: "guarded_parallel".to_string(),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            // Record that the action ran (it shouldn't if guard_in fails)
            guard_eval_clone.store(true, Ordering::SeqCst);
            Ok(StepOutput::new("action ran".to_string()))
        })),
        // Use a guard_in that will fail (file doesn't exist)
        guard_in: Guard::FileExists("/definitely/does/not/exist/guard_test".to_string()),
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "guard_in_test".to_string(),
        steps: vec![step1],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();

    // This should fail because guard_in fails
    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(
        result.is_err(),
        "Pipeline should fail when parallel step's guard_in fails"
    );

    // The action should NOT have run because guard_in blocked it
    assert!(
        !guard_evaluated.load(Ordering::SeqCst),
        "Action should not have executed when guard_in failed"
    );
}
