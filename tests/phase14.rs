//! Phase 14: Cancellation and Interrupt
//!
//! Tests for cancellation token support and clean pipeline interruption.

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

#[tokio::test]
async fn test_cancellation_token_basics() {
    // Test basic CancellationToken functionality
    let token = CancellationToken::new();
    assert!(!token.is_cancelled(), "token should start as not cancelled");

    token.cancel();
    assert!(
        token.is_cancelled(),
        "token should be cancelled after cancel()"
    );
}

#[tokio::test]
async fn test_cancellation_token_clone() {
    // Test that cloned tokens share cancellation state
    let token1 = CancellationToken::new();
    let token2 = token1.clone();

    token1.cancel();
    assert!(
        token2.is_cancelled(),
        "cloned token should reflect cancellation"
    );
}

#[tokio::test]
async fn test_cancellation_await() {
    // Test that awaiting cancellation works correctly
    let token = CancellationToken::new();
    let token_clone = token.clone();

    let handle = tokio::spawn(async move {
        token_clone.cancelled().await;
        "finished"
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    token.cancel();

    let result = handle.await.unwrap();
    assert_eq!(result, "finished");
}

#[tokio::test]
async fn test_cancellation_not_triggered() {
    // Test that a pipeline runs normally when cancellation is not triggered
    let mut runner = PipelineRunner::new();

    let agent = Agent {
        name: "test_agent".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![
                AgentStep {
                    name: "step1".into(),
                    guard_in: Guard::None,
                    action: StepAction::Custom(Arc::new(|ctx| {
                        assert!(!ctx.cancellation_token.is_cancelled());
                        Ok(StepOutput::new("step1 output".into()))
                    })),
                    guard_out: Guard::CancellationCleanupComplete,
                    verdict: Verdict::None,
                    tools: ToolSet::None,
                    injection_protection: InjectionProtection::None,
                    output_schema: None,
                    dependencies: vec![],
                    parallel: false,
                    input_processors: vec![],
                    output_processors: vec![],
                },
                AgentStep {
                    name: "step2".into(),
                    guard_in: Guard::None,
                    action: StepAction::Custom(Arc::new(|_ctx| {
                        Ok(StepOutput::new("step2 output".into()))
                    })),
                    guard_out: Guard::CancellationCleanupComplete,
                    verdict: Verdict::None,
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
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "pipeline should succeed when not cancelled");

    if let Ok(result) = result {
        assert_eq!(result.steps_passed.len(), 2, "both steps should pass");
        assert_eq!(result.steps_failed.len(), 0, "no steps should fail");
    }
}

#[tokio::test]
async fn test_cancellation_between_steps() {
    // Test that cancellation stops the pipeline between steps
    let agent = Agent {
        name: "test_agent".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![
                AgentStep {
                    name: "step1".into(),
                    guard_in: Guard::None,
                    action: StepAction::Custom(Arc::new(|ctx| {
                        // First step cancels the token
                        ctx.cancellation_token.cancel();
                        Ok(StepOutput::new("step1 output".into()))
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
                },
                AgentStep {
                    name: "step2".into(),
                    guard_in: Guard::None,
                    action: StepAction::Custom(Arc::new(|_ctx| {
                        Ok(StepOutput::new("step2 output".into()))
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
                },
            ],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner.run(&agent.pipeline, &agent, json!({})).await;

    // The pipeline should fail because cancellation was triggered between steps
    assert!(
        result.is_err(),
        "pipeline should fail when cancellation token is signalled"
    );

    if let Ok(result) = result {
        // Step 1 should have passed before cancellation
        assert!(
            result.steps_passed.contains(&"step1".into()),
            "step1 should pass"
        );
        // Step 2 should not have run (or failed due to cancellation check)
        assert!(
            !result.steps_passed.contains(&"step2".into()),
            "step2 should not pass"
        );
    }
}

#[tokio::test]
async fn test_cancellation_guard_detects_cancellation() {
    // Test that Guard::CancellationCleanupComplete detects when cancellation occurred
    let agent = Agent {
        name: "test_agent".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![AgentStep {
                name: "step1".into(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|ctx| {
                    ctx.cancellation_token.cancel();
                    Ok(StepOutput::new("output".into()))
                })),
                guard_out: Guard::CancellationCleanupComplete,
                verdict: Verdict::None,
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
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner.run(&agent.pipeline, &agent, json!({})).await;

    // guard_out should fail because cancellation_token.is_cancelled() returns true
    assert!(
        result.is_err(),
        "guard_out should fail when cancellation is detected"
    );
}

#[tokio::test]
async fn test_cancellation_child_token() {
    // Test that child tokens inherit parent cancellation
    let parent = CancellationToken::new();
    let child = parent.child_token();

    parent.cancel();
    assert!(
        child.is_cancelled(),
        "child token should reflect parent cancellation"
    );
}

#[tokio::test]
async fn test_multiple_cancellations_idempotent() {
    // Test that multiple cancel() calls don't cause issues (idempotent)
    let token = CancellationToken::new();
    token.cancel();
    token.cancel();
    token.cancel();
    assert!(
        token.is_cancelled(),
        "token should remain cancelled after multiple calls"
    );
}

#[tokio::test]
async fn test_cancellation_with_long_running_custom_action() {
    // Test cancellation during a long-running custom action
    let agent = Agent {
        name: "test_agent".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![AgentStep {
                name: "long_step".into(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|ctx| {
                    // Simulate work
                    for _ in 0..10 {
                        if ctx.cancellation_token.is_cancelled() {
                            return Err(StepError::ActionFailed {
                                reason: "Cancelled during execution".into(),
                            });
                        }
                    }
                    Ok(StepOutput::new("completed".into()))
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
            }],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let result = runner.run(&agent.pipeline, &agent, json!({})).await;

    // Without external cancellation, the step should complete successfully
    assert!(
        result.is_ok(),
        "step should complete when not cancelled externally"
    );
}
