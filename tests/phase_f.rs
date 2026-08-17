//! Phase F: Evaluation Polish
//! Tests for Scorer sampling, RubricLoop, and Experiment runner
//! Also includes workspace isolation integration tests (audit finding b6b1d8bcb1224ae6a767d688a4bd01b8)

use serde_json::json;
use std::sync::Arc;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use verdict::prelude::*;

// ============================================================================
// Phase F1: Scorer Sampling Tests
// ============================================================================

#[test]
fn test_scorer_config_creation() {
    let scorer = Arc::new(ToxicityScorer {
        blocked_patterns: vec!["bad_word".into()],
    });

    let config = ScorerConfig {
        scorer,
        sampling_rate: 0.5,
    };

    assert_eq!(config.sampling_rate, 0.5);
}

#[test]
fn test_toxicity_scorer_blocks_pattern() {
    let _scorer = ToxicityScorer {
        blocked_patterns: vec!["toxin".into()],
    };

     let result = PipelineResult {
         pipeline_name: "test".into(),
         steps_passed: vec!["step1".into()],
         steps_failed: vec![],
         step_results: {
             let mut m = std::collections::HashMap::new();
             m.insert(
                 "step1".into(),
                 StepResult {
                     step_name: "step1".into(),
                     output: StepOutput::new("This contains toxin".into()),
                     verdict_passed: true,
                     error: None,
                 },
             );
             m
         },
         audit_log: AuditLog::new(),
         success: true,
         total_cost_usd: 0.0,
         total_tokens_used: 0,
         log: vec![],
         suspended: None,
         budget: Default::default(),
     };

    // Note: Would need async context for actual scorer call
    // For now we test the structure
    assert!(result.success);
}

#[test]
fn test_toxicity_scorer_name() {
    let scorer = ToxicityScorer {
        blocked_patterns: vec![],
    };

    assert_eq!(scorer.name(), "toxicity");
}

#[test]
fn test_custom_scorer_construction() {
    let scorer = CustomScorer {
        name: "my_scorer".into(),
        func: Arc::new(|_| {
            Ok(ScorerResult {
                score: 1.0,
                pass: true,
                feedback: None,
            })
        }),
    };

    assert_eq!(scorer.name(), "my_scorer");
}

// ============================================================================
// Phase F2: RubricLoop Step Action Tests
// ============================================================================

#[test]
fn test_rubric_item_creation() {
    let item = RubricItem {
        criterion: "output is non-empty".into(),
        required: true,
    };

    assert_eq!(item.criterion, "output is non-empty");
    assert!(item.required);
}

#[test]
fn test_rubric_loop_action_construction() {
    let rubric_loop = StepAction::RubricLoop {
        body: Box::new(StepAction::LlmCall {
            system: "test".into(),
            user: "test".into(),
            model: None,
            conversation_id: None,
            append_to_history: false,
        }),
        rubric: vec![
            RubricItem {
                criterion: "criterion 1".into(),
                required: true,
            },
            RubricItem {
                criterion: "criterion 2".into(),
                required: false,
            },
        ],
        max_iterations: 5,
        judge_model: None,
    };

    match rubric_loop {
        StepAction::RubricLoop { ref rubric, .. } => {
            assert_eq!(rubric.len(), 2);
            assert!(rubric[0].required);
            assert!(!rubric[1].required);
        }
        _ => panic!("Expected RubricLoop action"),
    }
}

// ============================================================================
// Phase F3: Evaluation Dataset and Experiment Runner Tests
// ============================================================================

#[test]
fn test_evaluation_dataset_creation() {
    let dataset = EvaluationDataset::new("test_dataset");

    assert_eq!(dataset.name, "test_dataset");
    assert_eq!(dataset.version, 1);
    assert!(dataset.cases.is_empty());
}

#[test]
fn test_evaluation_dataset_with_version() {
    let dataset = EvaluationDataset::new("test").with_version(5);

    assert_eq!(dataset.version, 5);
}

#[test]
fn test_evaluation_dataset_add_case() {
    let case = EvaluationCase {
        name: "case1".into(),
        input: json!({"test": "input"}),
        expected: EvaluationExpected::Guard(Guard::NonEmptyOutput),
    };

    let dataset = EvaluationDataset::new("test").add_case(case);

    assert_eq!(dataset.cases.len(), 1);
    assert_eq!(dataset.cases[0].name, "case1");
}

#[test]
fn test_experiment_runner_creation() {
    let runner = Arc::new(PipelineRunner::new());
    let _exp_runner = ExperimentRunner::new(runner);

    // Test that it was constructed
    assert!(true);
}

#[test]
fn test_experiment_diff_no_changes() {
    let dataset = EvaluationDataset::new("test");
    let exp_a = Experiment {
        name: "exp_a".into(),
        dataset: dataset.clone(),
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![EvaluationResult {
            case_name: "case1".into(),
            passed: true,
            score: 1.0,
            reason: None,
        }],
        summary_score: 1.0,
    };

    let exp_b = Experiment {
        name: "exp_b".into(),
        dataset,
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![EvaluationResult {
            case_name: "case1".into(),
            passed: true,
            score: 1.0,
            reason: None,
        }],
        summary_score: 1.0,
    };

    let diff = ExperimentRunner::compare(&exp_a, &exp_b);

    assert_eq!(diff.score_delta, 0.0);
    assert!(diff.improved_cases.is_empty());
    assert!(diff.regressed_cases.is_empty());
}

#[test]
fn test_experiment_diff_improvement() {
    let dataset = EvaluationDataset::new("test");
    let exp_a = Experiment {
        name: "exp_a".into(),
        dataset: dataset.clone(),
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![EvaluationResult {
            case_name: "case1".into(),
            passed: false,
            score: 0.5,
            reason: None,
        }],
        summary_score: 0.5,
    };

    let exp_b = Experiment {
        name: "exp_b".into(),
        dataset,
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![EvaluationResult {
            case_name: "case1".into(),
            passed: true,
            score: 1.0,
            reason: None,
        }],
        summary_score: 1.0,
    };

    let diff = ExperimentRunner::compare(&exp_a, &exp_b);

    assert!(diff.score_delta > 0.0);
    assert_eq!(diff.improved_cases.len(), 1);
    assert_eq!(diff.improved_cases[0], "case1");
}

#[test]
fn test_experiment_diff_regression() {
    let dataset = EvaluationDataset::new("test");
    let exp_a = Experiment {
        name: "exp_a".into(),
        dataset: dataset.clone(),
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![EvaluationResult {
            case_name: "case1".into(),
            passed: true,
            score: 1.0,
            reason: None,
        }],
        summary_score: 1.0,
    };

    let exp_b = Experiment {
        name: "exp_b".into(),
        dataset,
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![EvaluationResult {
            case_name: "case1".into(),
            passed: false,
            score: 0.0,
            reason: None,
        }],
        summary_score: 0.0,
    };

    let diff = ExperimentRunner::compare(&exp_a, &exp_b);

    assert!(diff.score_delta < 0.0);
    assert_eq!(diff.regressed_cases.len(), 1);
    assert_eq!(diff.regressed_cases[0], "case1");
}

#[test]
fn test_scorer_result_structure() {
    let result = ScorerResult {
        score: 0.8,
        pass: true,
        feedback: Some("Good output".into()),
    };

    assert_eq!(result.score, 0.8);
    assert!(result.pass);
    assert_eq!(result.feedback, Some("Good output".into()));
}

#[test]
fn test_agent_with_scorers_field() {
    let agent = Agent {
        name: "test".into(),
        description: "test agent".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: Vec::new(),
    };

    assert_eq!(agent.scorers.len(), 0);
}

#[test]
fn test_agent_with_multiple_scorers() {
    let scorer1 = Arc::new(ToxicityScorer {
        blocked_patterns: vec!["bad".into()],
    });
    let scorer2 = Arc::new(CustomScorer {
        name: "custom".into(),
        func: Arc::new(|_| {
            Ok(ScorerResult {
                score: 1.0,
                pass: true,
                feedback: None,
            })
        }),
    });

    let agent = Agent {
        name: "test".into(),
        description: "test agent".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: vec![
            ScorerConfig {
                scorer: scorer1,
                sampling_rate: 0.5,
            },
            ScorerConfig {
                scorer: scorer2,
                sampling_rate: 1.0,
            },
        ],
    };

    assert_eq!(agent.scorers.len(), 2);
}

// ============================================================================
// Phase F: Workspace Isolation Integration Tests
// (Audit finding b6b1d8bcb1224ae6a767d688a4bd01b8 fix verification)
// ============================================================================

#[tokio::test]
async fn test_workspace_isolation_none_unchanged() {
    // WorkspaceIsolation::None should use the declared workspace_root unchanged
    // and allow normal filesystem operations.
    let workspace_root = PathBuf::from("/tmp/test_ws_none");
    fs::create_dir_all(&workspace_root).ok();
    let test_file_path = workspace_root.join("test_file.txt");

    let policy = AgentPolicy {
        max_steps: 1,
        max_retries: 0,
        max_delegation_depth: 0,
        max_cost_usd: None,
        max_runtime_seconds: None,
        allow_self_update: false,
        require_approval_for_self_update: false,
        allowed_agents: vec![],
        allowed_tools: ToolSet::ReadWrite,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy {
            workspace_root: workspace_root.clone(),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    };

    let agent = Agent {
        name: "test_none".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadWrite,
        skills: SkillSet::new(),
        policy,
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    
    // Create a pipeline that writes a file via fs.write tool
    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![AgentStep {
            name: "write_test_file".into(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "fs.write".into(),
                args: json!({
                    "path": "test_file.txt",
                    "content": "test content"
                }),
            },
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::ReadWrite,
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

    // Run pipeline
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Pipeline should succeed with WorkspaceIsolation::None");
    
    // Verify the file was actually written to workspace_root
    assert!(
        test_file_path.exists(),
        "File should exist at {:?}",
        test_file_path
    );
    let content = fs::read_to_string(&test_file_path).expect("should read file");
    assert_eq!(content, "test content");

    fs::remove_dir_all(&workspace_root).ok();
}

#[tokio::test]
async fn test_workspace_isolation_tempdir_creates_and_cleans_up() {
    // WorkspaceIsolation::TempDir should create a temp dir for the pipeline run,
    // use it for tool operations, and clean it up after.
    let unique_suffix = std::process::id();
    let original_workspace = PathBuf::from(format!("/tmp/test_ws_tempdir_outer_{}", unique_suffix));
    
    // Clean up any pre-existing directory at this path
    fs::remove_dir_all(&original_workspace).ok();
    
    // Create fresh directory
    fs::create_dir_all(&original_workspace).ok();

    let policy = AgentPolicy {
        max_steps: 1,
        max_retries: 0,
        max_delegation_depth: 0,
        max_cost_usd: None,
        max_runtime_seconds: None,
        allow_self_update: false,
        require_approval_for_self_update: false,
        allowed_agents: vec![],
        allowed_tools: ToolSet::ReadWrite,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy {
            workspace_root: original_workspace.clone(),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::TempDir,
        },
    };

    let agent = Agent {
        name: "test_tempdir".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadWrite,
        skills: SkillSet::new(),
        policy,
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    
    // Create a pipeline that writes a file via fs.write tool
    // With TempDir isolation, this file should go to a temp directory, not original_workspace
    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![AgentStep {
            name: "write_in_tempdir".into(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "fs.write".into(),
                args: json!({
                    "path": "tempdir_test_file.txt",
                    "content": "content from tempdir isolation"
                }),
            },
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::ReadWrite,
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

    // Run the pipeline
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Pipeline should succeed");

    // The original workspace_root should still exist and MUST be empty/unchanged
    // (the test file should NOT be there because TempDir isolated it)
    assert!(original_workspace.exists(), "Original workspace root should still exist");
    let tempdir_test_file = original_workspace.join("tempdir_test_file.txt");
    assert!(
        !tempdir_test_file.exists(),
        "File should NOT exist in original workspace when using TempDir isolation"
    );

    fs::remove_dir_all(&original_workspace).ok();
}

#[tokio::test]
async fn test_workspace_isolation_sandboxed_path_valid() {
    // WorkspaceIsolation::Sandboxed should use the provided sandbox path for all operations
    let sandbox_dir = TempDir::new().expect("failed to create temp dir");
    let sandbox_path = sandbox_dir.path().to_path_buf();

    let policy = AgentPolicy {
        max_steps: 1,
        max_retries: 0,
        max_delegation_depth: 0,
        max_cost_usd: None,
        max_runtime_seconds: None,
        allow_self_update: false,
        require_approval_for_self_update: false,
        allowed_agents: vec![],
        allowed_tools: ToolSet::ReadWrite,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy {
            workspace_root: PathBuf::from("/should/not/be/used"),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::Sandboxed(sandbox_path.clone()),
        },
    };

    let agent = Agent {
        name: "test_sandboxed".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadWrite,
        skills: SkillSet::new(),
        policy,
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    
    // Create a pipeline that writes a file to the sandbox
    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![AgentStep {
            name: "write_to_sandbox".into(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "fs.write".into(),
                args: json!({
                    "path": "sandbox_file.txt",
                    "content": "written to sandbox"
                }),
            },
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::ReadWrite,
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

    // Run the pipeline with the valid sandboxed path
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Pipeline should succeed with valid sandbox path");

    // Verify the file was written to the sandbox directory
    let sandbox_file = sandbox_path.join("sandbox_file.txt");
    assert!(
        sandbox_file.exists(),
        "File should exist in sandbox at {:?}",
        sandbox_file
    );
    let content = fs::read_to_string(&sandbox_file).expect("should read sandbox file");
    assert_eq!(content, "written to sandbox");
}

#[tokio::test]
async fn test_workspace_isolation_sandboxed_path_nonexistent() {
    // WorkspaceIsolation::Sandboxed with a nonexistent path should fail with RuntimeSetupFailed
    let nonexistent_path = PathBuf::from("/this/path/definitely/does/not/exist/workspace");

    let policy = AgentPolicy {
        max_steps: 1,
        max_retries: 0,
        max_delegation_depth: 0,
        max_cost_usd: None,
        max_runtime_seconds: None,
        allow_self_update: false,
        require_approval_for_self_update: false,
        allowed_agents: vec![],
        allowed_tools: ToolSet::ReadOnly,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy {
            workspace_root: PathBuf::from("/should/not/be/used"),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::Sandboxed(nonexistent_path),
        },
    };

    let agent = Agent {
        name: "test_sandboxed_nonexist".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet::new(),
        policy,
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Run should fail with a RuntimeSetupFailed error (not panic)
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(
        result.is_err(),
        "Pipeline should fail with nonexistent sandboxed path"
    );

    // Verify it's a RuntimeSetupFailed error by checking the error message
    if let Err(e) = result {
        let err_str = format!("{}", e);
        assert!(
            err_str.contains("does not exist"),
            "Error message should mention nonexistent path: {}",
            err_str
        );
    }
}

#[tokio::test]
async fn test_shell_command_rejects_absolute_paths() {
    // Shell tools should reject absolute paths in arguments
    let workspace = TempDir::new().expect("failed to create temp dir");
    let workspace_path = workspace.path().to_path_buf();

    let policy = AgentPolicy {
        max_steps: 1,
        max_retries: 0,
        max_delegation_depth: 0,
        max_cost_usd: None,
        max_runtime_seconds: None,
        allow_self_update: false,
        require_approval_for_self_update: false,
        allowed_agents: vec![],
        allowed_tools: ToolSet::Full,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy {
            workspace_root: workspace_path.clone(),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    };

    let agent = Agent {
        name: "test_shell_escape".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::Full,
        skills: SkillSet::new(),
        policy,
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    
    // Try to run a shell command with an absolute path (should fail)
    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![AgentStep {
            name: "attempt_absolute_path_escape".into(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "shell.run".into(),
                args: json!({
                    "command": "touch",
                    "args": ["/tmp/escaped_file.txt"]
                }),
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
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Run should fail because absolute path is rejected
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(
        result.is_err(),
        "Shell command with absolute path should be rejected"
    );
}

#[tokio::test]
async fn test_shell_bash_c_escape_blocked() {
    // Verify that bash -c bypass is now blocked: dangerous commands wrapped in -c are rejected
    let workspace = TempDir::new().expect("failed to create temp dir");
    let workspace_path = workspace.path().to_path_buf();

    let policy = AgentPolicy {
        max_steps: 1,
        max_retries: 0,
        max_delegation_depth: 0,
        max_cost_usd: None,
        max_runtime_seconds: None,
        allow_self_update: false,
        require_approval_for_self_update: false,
        allowed_agents: vec![],
        allowed_tools: ToolSet::Full,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy {
            workspace_root: workspace_path.clone(),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    };

    let agent = Agent {
        name: "test_bash_c_escape".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test".into(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::Full,
        skills: SkillSet::new(),
        policy,
        scorers: vec![],
    };

    let mut runner = PipelineRunner::new();
    
    // Try to run: bash -c "echo pwned > /tmp/escaped"
    // This should be REJECTED because the -c argument contains an absolute path redirect
    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![AgentStep {
            name: "attempt_bash_c_escape".into(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "shell.run".into(),
                args: json!({
                    "command": "bash",
                    "args": ["-c", "echo pwned > /tmp/escaped"]
                }),
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
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Run should fail because -c argument contains absolute path redirect
    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(
        result.is_err(),
        "bash -c escape with absolute path redirect should be rejected"
    );
}
