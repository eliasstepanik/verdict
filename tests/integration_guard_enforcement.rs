//! Integration tests: Guard chains and security enforcement in live pipeline runs
//!
//! All tests run through PipelineRunner::run(), not GuardEngine::evaluate() directly.
//! Tests verify guard enforcement at both guard_in and guard_out phases.

use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use tempfile::TempDir;
use verdict::prelude::*;

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ helpers Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

fn make_step(name: &str, guard_in: Guard, action: StepAction, guard_out: Guard) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in,
        action,
        guard_out,
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

fn ok_action(output: &'static str) -> StepAction {
    StepAction::Custom(Arc::new(move |_ctx| Ok(StepOutput::new(output.into()))))
}

#[allow(dead_code)]
fn fail_action(reason: &'static str) -> StepAction {
    StepAction::Custom(Arc::new(move |_ctx| {
        Err(StepError::ActionFailed {
            reason: reason.into(),
        })
    }))
}

fn abort_pipeline(name: &str, steps: Vec<AgentStep>) -> (Pipeline, Agent) {
    let pipeline = Pipeline {
        name: name.into(),
        steps,
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let agent = Agent {
        name: "t".into(),
        description: "t".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy: AgentPolicy {
            allowed_tools: ToolSet::Full,
            ..Default::default()
        },
        scorers: vec![],
    };
    (pipeline, agent)
}

#[allow(dead_code)]
fn skip_pipeline(name: &str, steps: Vec<AgentStep>) -> (Pipeline, Agent) {
    let pipeline = Pipeline {
        name: name.into(),
        steps,
        on_failure: FailureMode::Skip,
        max_retries: 0,
    };
    let agent = Agent {
        name: "t".into(),
        description: "t".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy: AgentPolicy {
            allowed_tools: ToolSet::Full,
            ..Default::default()
        },
        scorers: vec![],
    };
    (pipeline, agent)
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 1: AllOf - all three guards pass Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_allof_all_guards_pass() {
    let step = make_step(
        "composed",
        Guard::None,
        ok_action("line1\nline2\nline3"),
        Guard::AllOf(vec![
            Guard::NonEmptyOutput,
            Guard::MaxOutputBytes(100),
            Guard::MaxLines(3),
        ]),
    );
    let (pipeline, agent) = abort_pipeline("allof_pass", vec![step]);
    let result = PipelineRunner::new()
        .run(&pipeline, &agent, json!({}))
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(result.steps_passed, vec!["composed".to_string()]);
    assert!(result.step_results["composed"].verdict_passed);
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 2: AllOf - one guard fails, pipeline aborts Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_allof_one_guard_fails_aborts_pipeline() {
    let downstream_ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&downstream_ran);

    let step_a = make_step(
        "oversized",
        Guard::None,
        ok_action("this output is more than five bytes long"),
        Guard::AllOf(vec![Guard::NonEmptyOutput, Guard::MaxOutputBytes(5)]),
    );
    let step_b = AgentStep {
        name: "downstream".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |_ctx| {
            flag.store(true, Ordering::SeqCst);
            Ok(StepOutput::new("ran".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let (pipeline, agent) = abort_pipeline("allof_fail", vec![step_a, step_b]);
    let err = PipelineRunner::new()
        .run(&pipeline, &agent, json!({}))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            PipelineError::GuardFailed {
                phase: GuardPhase::Out,
                ..
            }
        ),
        "expected GuardFailed(Out), got {err:?}"
    );
    assert!(
        !downstream_ran.load(Ordering::SeqCst),
        "downstream must not run after abort"
    );
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 3: AnyOf - first fails, second passes Ã¢â€ â€™ step proceeds Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_anyof_first_fails_second_passes_proceeds() {
    let step = make_step(
        "any_of_step",
        Guard::None,
        ok_action("plain text"),
        Guard::AnyOf(vec![Guard::ValidJson, Guard::NonEmptyOutput]),
    );
    let (pipeline, agent) = abort_pipeline("anyof_p", vec![step]);
    let result = PipelineRunner::new()
        .run(&pipeline, &agent, json!({}))
        .await
        .unwrap();

    assert!(result.success);
    assert_eq!(result.step_results["any_of_step"].output.raw, "plain text");
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 4: Not(ValidJson) passes on plain text, fails on JSON Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_not_validjson_passes_on_plain_text_fails_on_json() {
    async fn run_with_output(output: &'static str) -> Result<PipelineResult, PipelineError> {
        let step = AgentStep {
            name: "not_json".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| Ok(StepOutput::new(output.into())))),
            guard_out: Guard::Not(Box::new(Guard::ValidJson)),
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
            name: "p".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = Agent {
            name: "a".into(),
            description: "a".into(),
            pipeline: pipeline.clone(),
            tools: ToolSet::Full,
            skills: SkillSet::default(),
            policy: AgentPolicy {
                allowed_tools: ToolSet::Full,
                ..Default::default()
            },
            scorers: vec![],
        };
        PipelineRunner::new()
            .run(&pipeline, &agent, json!({}))
            .await
    }

    assert!(
        run_with_output("hello world").await.is_ok(),
        "plain text should pass Not(ValidJson)"
    );
    assert!(
        matches!(
            run_with_output(r#"{"k":"v"}"#).await,
            Err(PipelineError::GuardFailed {
                phase: GuardPhase::Out,
                ..
            })
        ),
        "valid JSON should fail Not(ValidJson)"
    );
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 5: guard_in failure blocks action execution (counter stays at 0) Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_guard_in_failure_blocks_action_execution() {
    let action_calls = Arc::new(AtomicU32::new(0));
    let counter = Arc::clone(&action_calls);

    // guard_in = ValidJson but ctx has no output Ã¢â€ â€™ ValidJson fails (checks output.raw)
    // Actually, guard_in is evaluated with the step context BEFORE action, so output is None.
    // NonEmptyOutput guard on input: use a guard that always fails deterministically.
    // Use MaxOutputBytes(0) Ã¢â‚¬â€ any output (even empty from ctx.output=None) will be evaluated.
    // Simplest: use a Custom guard.
    let step = AgentStep {
        name: "protected".into(),
        guard_in: Guard::Custom(Arc::new(|_ctx| {
            Err(GuardError::Failed {
                guard: "CustomAlwaysFail".into(),
                reason: "guard_in always fails".into(),
            })
        })),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(StepOutput::new("ran".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let (pipeline, agent) = abort_pipeline("guard_in_block", vec![step]);
    let err = PipelineRunner::new()
        .run(&pipeline, &agent, json!({}))
        .await
        .unwrap_err();

    assert!(
        matches!(err, PipelineError::GuardFailed { phase: GuardPhase::In, ref step, .. } if step == "protected"),
        "expected GuardFailed(In) at protected, got {err:?}"
    );
    assert_eq!(
        action_calls.load(Ordering::SeqCst),
        0,
        "action must NOT run when guard_in fails"
    );
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 6: guard_out fails after action already executed Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_guard_out_fails_after_action_executes() {
    let action_ran = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&action_ran);

    let step = AgentStep {
        name: "oversized".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |_ctx| {
            flag.store(true, Ordering::SeqCst);
            Ok(StepOutput::new(
                "this output is much longer than five bytes".into(),
            ))
        })),
        guard_out: Guard::MaxOutputBytes(5),
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let (pipeline, agent) = abort_pipeline("guard_out_fail", vec![step]);
    let err = PipelineRunner::new()
        .run(&pipeline, &agent, json!({}))
        .await
        .unwrap_err();

    assert!(
        matches!(
            err,
            PipelineError::GuardFailed {
                phase: GuardPhase::Out,
                ..
            }
        ),
        "expected GuardFailed(Out), got {err:?}"
    );
    assert!(
        action_ran.load(Ordering::SeqCst),
        "action must have executed before guard_out"
    );
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 7: FileExists guard verifies prior step's file write Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_fileexists_guard_reads_prior_step_artifact() {
    let tmp = TempDir::new().unwrap();
    let workspace = tmp.path().to_path_buf();
    let artifact = workspace.join("artifact.txt");
    let artifact_clone = artifact.clone();

    let step_a = AgentStep {
        name: "write_file".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |_ctx| {
            std::fs::write(&artifact_clone, "hello integration test").map_err(|e| {
                StepError::ActionFailed {
                    reason: format!("write failed: {e}"),
                }
            })?;
            Ok(StepOutput::new("wrote artifact".into()))
        })),
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let step_b = AgentStep {
        name: "consume_file".into(),
        guard_in: Guard::FileExists("artifact.txt".into()),
        action: StepAction::Custom(Arc::new(|_| Ok(StepOutput::new("consumed".into())))),
        guard_out: Guard::None,
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
        name: "fs_handoff".into(),
        steps: vec![step_a, step_b],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let mut policy = AgentPolicy::default();
    policy.filesystem_policy = FilesystemPolicy {
        workspace_root: workspace,
        ..Default::default()
    };
    policy.allowed_tools = ToolSet::Full;
    let agent = Agent {
        name: "a".into(),
        description: "a".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    };

    let result = PipelineRunner::new()
        .run(&pipeline, &agent, json!({}))
        .await
        .unwrap();
    assert!(result.success);
    assert_eq!(
        result.steps_passed,
        vec!["write_file".to_string(), "consume_file".to_string()]
    );
    assert!(artifact.exists());
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 8: MaxLines(3) blocks 4-line output, passes 3-line output Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_maxlines_blocks_four_lines_passes_three() {
    async fn run_with_output(output: &'static str) -> Result<PipelineResult, PipelineError> {
        let step = AgentStep {
            name: "linecheck".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| Ok(StepOutput::new(output.into())))),
            guard_out: Guard::MaxLines(3),
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
            name: "p".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = Agent {
            name: "a".into(),
            description: "a".into(),
            pipeline: pipeline.clone(),
            tools: ToolSet::Full,
            skills: SkillSet::default(),
            policy: AgentPolicy {
                allowed_tools: ToolSet::Full,
                ..Default::default()
            },
            scorers: vec![],
        };
        PipelineRunner::new()
            .run(&pipeline, &agent, json!({}))
            .await
    }

    // 3 lines Ã¢â‚¬â€ passes
    assert!(
        run_with_output("a\nb\nc").await.is_ok(),
        "3 lines should pass MaxLines(3)"
    );

    // 4 lines Ã¢â‚¬â€ fails
    let err = run_with_output("a\nb\nc\nd").await.unwrap_err();
    assert!(
        matches!(
            err,
            PipelineError::GuardFailed {
                phase: GuardPhase::Out,
                ..
            }
        ),
        "4 lines should fail MaxLines(3), got {err:?}"
    );
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 9: MatchesSchema validates structured JSON output Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_matches_schema_validates_structured_json_output() {
    let schema = json!({
        "type": "object",
        "required": ["name", "age"],
        "properties": {
            "name": { "type": "string" },
            "age":  { "type": "integer" }
        }
    });

    async fn run_with_output(
        output: &'static str,
        schema: serde_json::Value,
    ) -> Result<PipelineResult, PipelineError> {
        let step = AgentStep {
            name: "schema_check".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| Ok(StepOutput::new(output.into())))),
            guard_out: Guard::MatchesSchema(schema),
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
            name: "p".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = Agent {
            name: "a".into(),
            description: "a".into(),
            pipeline: pipeline.clone(),
            tools: ToolSet::Full,
            skills: SkillSet::default(),
            policy: AgentPolicy {
                allowed_tools: ToolSet::Full,
                ..Default::default()
            },
            scorers: vec![],
        };
        PipelineRunner::new()
            .run(&pipeline, &agent, json!({}))
            .await
    }

    // Valid: both required fields present
    assert!(
        run_with_output(r#"{"name":"alice","age":30}"#, schema.clone())
            .await
            .is_ok(),
        "schema-valid JSON should pass"
    );

    // Invalid: missing required 'age' field
    assert!(
        matches!(
            run_with_output(r#"{"name":"alice"}"#, schema.clone()).await,
            Err(PipelineError::GuardFailed {
                phase: GuardPhase::Out,
                ..
            })
        ),
        "missing required field should fail MatchesSchema"
    );
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 10: NoSecretsInOutput passes on clean, fails on leaked key Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_no_secrets_in_output_clean_passes_leaked_fails() {
    async fn run_with_output(output: &'static str) -> Result<PipelineResult, PipelineError> {
        let step = AgentStep {
            name: "secret_scan".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| Ok(StepOutput::new(output.into())))),
            guard_out: Guard::NoSecretsInOutput,
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
            name: "p".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = Agent {
            name: "a".into(),
            description: "a".into(),
            pipeline: pipeline.clone(),
            tools: ToolSet::Full,
            skills: SkillSet::default(),
            policy: AgentPolicy {
                allowed_tools: ToolSet::Full,
                ..Default::default()
            },
            scorers: vec![],
        };
        PipelineRunner::new()
            .run(&pipeline, &agent, json!({}))
            .await
    }

    assert!(
        run_with_output("the answer is 42").await.is_ok(),
        "clean output should pass"
    );

    // OpenAI-style key Ã¢â‚¬â€ should be detected as a secret
    let result = run_with_output("api key: sk-proj-abcdef1234567890abcdef1234567890abcdef12").await;
    assert!(
        matches!(
            result,
            Err(PipelineError::GuardFailed {
                phase: GuardPhase::Out,
                ..
            })
        ),
        "leaked key should fail NoSecretsInOutput"
    );
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 11: StepPassed gates downstream execution Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_step_passed_gates_downstream_execution() {
    async fn run(make_a_fail: bool, b_ran: Arc<AtomicU32>) -> PipelineResult {
        let should_fail = make_a_fail;
        let step_a = AgentStep {
            name: "step_a".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| {
                if should_fail {
                    Err(StepError::ActionFailed {
                        reason: "forced".into(),
                    })
                } else {
                    Ok(StepOutput::new("a_ok".into()))
                }
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        };
        let counter = Arc::clone(&b_ran);
        let step_b = AgentStep {
            name: "step_b".into(),
            guard_in: Guard::StepPassed("step_a".into()),
            action: StepAction::Custom(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(StepOutput::new("b_ok".into()))
            })),
            guard_out: Guard::None,
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
            name: "p".into(),
            steps: vec![step_a, step_b],
            on_failure: FailureMode::Skip,
            max_retries: 0,
        };
        let agent = Agent {
            name: "a".into(),
            description: "a".into(),
            pipeline: pipeline.clone(),
            tools: ToolSet::Full,
            skills: SkillSet::default(),
            policy: AgentPolicy {
                allowed_tools: ToolSet::Full,
                ..Default::default()
            },
            scorers: vec![],
        };
        PipelineRunner::new()
            .run(&pipeline, &agent, json!({}))
            .await
            .unwrap()
    }

    // A passes Ã¢â€ â€™ B runs
    let b_count = Arc::new(AtomicU32::new(0));
    let r = run(false, Arc::clone(&b_count)).await;
    assert_eq!(
        b_count.load(Ordering::SeqCst),
        1,
        "step_b must run when step_a passed"
    );
    assert!(r.steps_passed.contains(&"step_b".to_string()));

    // A fails Ã¢â€ â€™ B's guard_in (StepPassed) fails Ã¢â€ â€™ B blocked
    let b_count2 = Arc::new(AtomicU32::new(0));
    let r2 = run(true, Arc::clone(&b_count2)).await;
    assert_eq!(
        b_count2.load(Ordering::SeqCst),
        0,
        "step_b must NOT run when step_a failed"
    );
    assert!(r2.steps_failed.contains(&"step_a".to_string()));
    assert!(r2.steps_failed.contains(&"step_b".to_string()));
}

// Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬ Test 12: StepFailed gates recovery step Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬Ã¢â€â‚¬

#[tokio::test]
async fn test_step_failed_gates_recovery_step() {
    async fn run(make_a_fail: bool, recovery_ran: Arc<AtomicU32>) -> PipelineResult {
        let should_fail = make_a_fail;
        let step_a = AgentStep {
            name: "step_a".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(move |_| {
                if should_fail {
                    Err(StepError::ActionFailed {
                        reason: "forced".into(),
                    })
                } else {
                    Ok(StepOutput::new("ok".into()))
                }
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        };
        let counter = Arc::clone(&recovery_ran);
        let recovery = AgentStep {
            name: "recovery".into(),
            guard_in: Guard::StepFailed("step_a".into()),
            action: StepAction::Custom(Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(StepOutput::new("recovered".into()))
            })),
            guard_out: Guard::None,
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
            name: "p".into(),
            steps: vec![step_a, recovery],
            on_failure: FailureMode::Skip,
            max_retries: 0,
        };
        let agent = Agent {
            name: "a".into(),
            description: "a".into(),
            pipeline: pipeline.clone(),
            tools: ToolSet::Full,
            skills: SkillSet::default(),
            policy: AgentPolicy {
                allowed_tools: ToolSet::Full,
                ..Default::default()
            },
            scorers: vec![],
        };
        PipelineRunner::new()
            .run(&pipeline, &agent, json!({}))
            .await
            .unwrap()
    }

    // A fails Ã¢â€ â€™ recovery runs
    let rec_count = Arc::new(AtomicU32::new(0));
    let r = run(true, Arc::clone(&rec_count)).await;
    assert_eq!(
        rec_count.load(Ordering::SeqCst),
        1,
        "recovery must run when step_a failed"
    );
    assert!(r.steps_passed.contains(&"recovery".to_string()));
    assert!(r.steps_failed.contains(&"step_a".to_string()));

    // A passes Ã¢â€ â€™ recovery's guard_in (StepFailed) fails Ã¢â€ â€™ recovery blocked
    let rec_count2 = Arc::new(AtomicU32::new(0));
    let r2 = run(false, Arc::clone(&rec_count2)).await;
    assert_eq!(
        rec_count2.load(Ordering::SeqCst),
        0,
        "recovery must NOT run when step_a passed"
    );
    assert!(r2.steps_passed.contains(&"step_a".to_string()));
    assert!(r2.steps_failed.contains(&"recovery".to_string()));
}
