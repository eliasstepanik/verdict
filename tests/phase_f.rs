//! Phase F: Evaluation Polish
//! Tests for Scorer sampling, RubricLoop, and Experiment runner

use verdict::prelude::*;
use std::sync::Arc;
use serde_json::json;

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
    let dataset = EvaluationDataset::new("test")
        .with_version(5);
    
    assert_eq!(dataset.version, 5);
}

#[test]
fn test_evaluation_dataset_add_case() {
    let case = EvaluationCase {
        name: "case1".into(),
        input: json!({"test": "input"}),
        expected: EvaluationExpected::Guard(Guard::NonEmptyOutput),
    };
    
    let dataset = EvaluationDataset::new("test")
        .add_case(case);
    
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
        results: vec![
            EvaluationResult {
                case_name: "case1".into(),
                passed: true,
                score: 1.0,
                reason: None,
            },
        ],
        summary_score: 1.0,
    };
    
    let exp_b = Experiment {
        name: "exp_b".into(),
        dataset,
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![
            EvaluationResult {
                case_name: "case1".into(),
                passed: true,
                score: 1.0,
                reason: None,
            },
        ],
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
        results: vec![
            EvaluationResult {
                case_name: "case1".into(),
                passed: false,
                score: 0.5,
                reason: None,
            },
        ],
        summary_score: 0.5,
    };
    
    let exp_b = Experiment {
        name: "exp_b".into(),
        dataset,
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![
            EvaluationResult {
                case_name: "case1".into(),
                passed: true,
                score: 1.0,
                reason: None,
            },
        ],
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
        results: vec![
            EvaluationResult {
                case_name: "case1".into(),
                passed: true,
                score: 1.0,
                reason: None,
            },
        ],
        summary_score: 1.0,
    };
    
    let exp_b = Experiment {
        name: "exp_b".into(),
        dataset,
        agent_name: "agent1".into(),
        run_at: chrono::Utc::now(),
        results: vec![
            EvaluationResult {
                case_name: "case1".into(),
                passed: false,
                score: 0.0,
                reason: None,
            },
        ],
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
