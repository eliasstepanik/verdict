//! Phase 8 — Self-Improvement Tests
//! Comprehensive test suite for evaluation suites and self-update system

use verdict::prelude::*;
use serde_json::json;

// ============================================================================
// EvaluationSuite Tests
// ============================================================================

#[test]
fn test_evaluation_suite_construction() {
    let suite = EvaluationSuite {
        name: "test_suite".to_string(),
        cases: vec![],
        minimum_score: 0.8,
    };

    assert_eq!(suite.name, "test_suite");
    assert_eq!(suite.cases.len(), 0);
    assert_eq!(suite.minimum_score, 0.8);
}

#[test]
fn test_evaluation_case_construction() {
    let case = EvaluationCase {
        name: "test_case".to_string(),
        input: json!({"key": "value"}),
        expected: EvaluationExpected::Exact(json!({"result": "ok"})),
    };

    assert_eq!(case.name, "test_case");
    assert_eq!(case.input, json!({"key": "value"}));
}

#[test]
fn test_evaluation_result_construction() {
    let result = EvaluationResult {
        case_name: "test_case".to_string(),
        passed: true,
        score: 1.0,
        reason: None,
    };

    assert_eq!(result.case_name, "test_case");
    assert!(result.passed);
    assert_eq!(result.score, 1.0);
    assert!(result.reason.is_none());
}

#[test]
fn test_evaluation_result_with_reason() {
    let result = EvaluationResult {
        case_name: "test_case".to_string(),
        passed: false,
        score: 0.0,
        reason: Some("test failed".to_string()),
    };

    assert!(!result.passed);
    assert_eq!(result.score, 0.0);
    assert_eq!(result.reason, Some("test failed".to_string()));
}

#[test]
fn test_evaluation_suite_result_construction() {
    let suite_result = EvaluationSuiteResult {
        suite_name: "test_suite".to_string(),
        results: vec![],
        overall_score: 1.0,
        passed: true,
    };

    assert_eq!(suite_result.suite_name, "test_suite");
    assert_eq!(suite_result.results.len(), 0);
    assert_eq!(suite_result.overall_score, 1.0);
    assert!(suite_result.passed);
}

#[test]
fn test_evaluation_suite_result_score_calculation() {
    let results = vec![
        EvaluationResult {
            case_name: "case1".to_string(),
            passed: true,
            score: 1.0,
            reason: None,
        },
        EvaluationResult {
            case_name: "case2".to_string(),
            passed: false,
            score: 0.0,
            reason: None,
        },
    ];

    let overall = results.iter().map(|r| r.score).sum::<f64>() / results.len() as f64;
    assert_eq!(overall, 0.5);
}

#[test]
fn test_evaluation_suite_zero_cases() {
    let suite = EvaluationSuite {
        name: "empty_suite".to_string(),
        cases: vec![],
        minimum_score: 0.8,
    };

    let result = EvaluationSuiteResult {
        suite_name: suite.name.clone(),
        results: vec![],
        overall_score: 1.0,
        passed: true,
    };

    assert!(result.passed);
    assert_eq!(result.overall_score, 1.0);
}

// ============================================================================
// SelfUpdateEngine Tests
// ============================================================================

#[test]
fn test_self_update_config_construction() {
    let config = SelfUpdateConfig {
        allowed_paths: vec!["src/".to_string()],
        forbidden_paths: vec!["Cargo.toml".to_string()],
        require_approval: true,
        sandbox_dir: None,
        run_eval_after: true,
    };

    assert_eq!(config.allowed_paths.len(), 1);
    assert_eq!(config.forbidden_paths.len(), 1);
    assert!(config.require_approval);
}

#[test]
fn test_self_update_config_default() {
    let config = SelfUpdateConfig::default();

    assert!(!config.allowed_paths.is_empty());
    assert!(!config.forbidden_paths.is_empty());
    assert!(config.require_approval);
}

#[test]
fn test_self_update_proposal_construction() {
    let proposal = SelfUpdateProposal {
        patch: "--- a/src/test.rs\n+++ b/src/test.rs\n@@ -1,1 +1,2 @@".to_string(),
        summary: "improve performance".to_string(),
        risk_level: RiskLevel::Low,
    };

    assert_eq!(proposal.summary, "improve performance");
    assert_eq!(proposal.risk_level, RiskLevel::Low);
}

#[test]
fn test_self_update_validate_proposal_empty_patch() {
    let proposal = SelfUpdateProposal {
        patch: "".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_err());
    match result {
        Err(SelfUpdateError::EmptyPatch) => (),
        _ => panic!("expected EmptyPatch error"),
    }
}

#[test]
fn test_self_update_validate_proposal_not_a_diff() {
    let proposal = SelfUpdateProposal {
        patch: "this is not a diff".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_err());
    match result {
        Err(SelfUpdateError::InvalidDiff) => (),
        _ => panic!("expected InvalidDiff error"),
    }
}

#[test]
fn test_self_update_validate_proposal_forbidden_path() {
    let proposal = SelfUpdateProposal {
        patch: "--- a/Cargo.toml\n+++ b/Cargo.toml\n@@ -1,1 +1,1 @@".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_err());
    match result {
        Err(SelfUpdateError::ForbiddenPath { .. }) => (),
        _ => panic!("expected ForbiddenPath error"),
    }
}

#[test]
fn test_self_update_validate_proposal_valid_diff() {
    let proposal = SelfUpdateProposal {
        patch: "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,2 @@\n content".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Low,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_ok());
}

#[test]
fn test_self_update_validate_proposal_valid_with_three_plus_markers() {
    let proposal = SelfUpdateProposal {
        patch: "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,2 @@\n-old line\n+new line".to_string(),
        summary: "test".to_string(),
        risk_level: RiskLevel::Medium,
    };
    let config = SelfUpdateConfig::default();

    let result = SelfUpdateEngine::validate_proposal(&proposal, &config);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_self_update_apply_in_sandbox_writes_patch() {
    let temp_dir = std::env::temp_dir().join("verdict_test_sandbox");
    let _ = std::fs::remove_dir_all(&temp_dir);
    std::fs::create_dir_all(&temp_dir).unwrap();

    let patch = "--- a/src/agents/test.rs\n+++ b/src/agents/test.rs\n@@ -1,1 +1,2 @@\n content";
    let workspace_root = std::env::current_dir().unwrap();

    let result = SelfUpdateEngine::apply_in_sandbox(patch, &temp_dir, &workspace_root).await;

    // The function writes the patch file before calling git apply.
    // Check that patch.diff was written regardless of git apply outcome.
    let patch_file = temp_dir.join("patch.diff");
    assert!(patch_file.exists(), "patch.diff should be written to sandbox dir");

    let written_patch = std::fs::read_to_string(&patch_file).unwrap();
    assert_eq!(written_patch.trim(), patch.trim(), "written patch content should match input");

    // The git apply step may fail in the test environment (no real git repo in temp_dir),
    // which is expected. The important invariant is that the function attempted it.
    // If git is available and the dry-run fails, we get PatchApplyFailed — that is correct.
    // If git is not installed, we get an IoError. Both outcomes are acceptable.
    match result {
        Ok(()) => { /* git apply succeeded unexpectedly — also fine */ }
        Err(SelfUpdateError::PatchApplyFailed(_)) => { /* expected: git apply dry-run rejected the patch */ }
        Err(SelfUpdateError::Io(_)) => { /* expected: git not installed or other I/O issue */ }
        Err(e) => panic!("unexpected error from apply_in_sandbox: {:?}", e),
    }

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_self_update_version_agent_creates_version() {
    let agent = Agent {
        name: "test_agent".to_string(),
        description: "A test agent".to_string(),
        pipeline: Pipeline {
            name: "test_pipeline".to_string(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
    };

    let version = SelfUpdateEngine::version_agent(&agent, "improved performance", Some(0.95));

    assert_eq!(version.agent_name, "test_agent");
    assert_eq!(version.change_summary, "improved performance");
    assert_eq!(version.evaluation_score, Some(0.95));
    assert!(version.parent_version.is_none());
}

#[test]
fn test_self_update_version_agent_name_matches() {
    let agent = Agent {
        name: "my_agent".to_string(),
        description: "Test".to_string(),
        pipeline: Pipeline {
            name: "test".to_string(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
    };

    let version = SelfUpdateEngine::version_agent(&agent, "test", None);
    assert_eq!(version.agent_name, "my_agent");
}

#[test]
fn test_self_update_version_agent_has_eval_score() {
    let agent = Agent {
        name: "test_agent".to_string(),
        description: "A test agent".to_string(),
        pipeline: Pipeline {
            name: "test_pipeline".to_string(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
    };

    let version_with_score =
        SelfUpdateEngine::version_agent(&agent, "improved", Some(0.85));
    let version_without_score = SelfUpdateEngine::version_agent(&agent, "improved", None);

    assert_eq!(version_with_score.evaluation_score, Some(0.85));
    assert!(version_without_score.evaluation_score.is_none());
}

// ============================================================================
// AgentVersion Tests
// ============================================================================

#[test]
fn test_agent_version_construction() {
    let version = AgentVersion {
        agent_name: "test".to_string(),
        version: "20240101120000".to_string(),
        parent_version: None,
        created_at: chrono::Utc::now(),
        change_summary: "initial version".to_string(),
        git_commit: None,
        evaluation_score: None,
    };

    assert_eq!(version.agent_name, "test");
    assert_eq!(version.version, "20240101120000");
    assert!(version.parent_version.is_none());
}

#[test]
fn test_agent_version_with_parent() {
    let version = AgentVersion {
        agent_name: "test".to_string(),
        version: "20240101120001".to_string(),
        parent_version: Some("20240101120000".to_string()),
        created_at: chrono::Utc::now(),
        change_summary: "v2".to_string(),
        git_commit: None,
        evaluation_score: None,
    };

    assert_eq!(version.parent_version, Some("20240101120000".to_string()));
}

#[test]
fn test_agent_version_with_eval_score() {
    let version = AgentVersion {
        agent_name: "test".to_string(),
        version: "20240101120001".to_string(),
        parent_version: None,
        created_at: chrono::Utc::now(),
        change_summary: "improved".to_string(),
        git_commit: None,
        evaluation_score: Some(0.92),
    };

    assert_eq!(version.evaluation_score, Some(0.92));
}

// ============================================================================
// Guard Phase 8 Tests
// ============================================================================

#[tokio::test]
async fn test_guard_evaluation_improves_or_equal_passes_with_score() {
    use verdict::guard::{GuardEngine};

    let mut ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    ctx.output = Some(StepOutput::new(r#"{"eval_score": 0.85}"#.to_string()));

    let result = GuardEngine::evaluate(&Guard::EvaluationImprovesOrEqual, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_evaluation_improves_or_equal_passes_no_output() {
    use verdict::guard::GuardEngine;

    let ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    let result = GuardEngine::evaluate(&Guard::EvaluationImprovesOrEqual, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_agent_version_created_passes_with_version() {
    use verdict::guard::GuardEngine;

    let mut ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    ctx.output = Some(StepOutput::new(r#"{"version": "20240101120000", "agent_name": "test"}"#.to_string()));

    let result = GuardEngine::evaluate(&Guard::AgentVersionCreated, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_agent_version_created_passes_no_output() {
    use verdict::guard::GuardEngine;

    let ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    let result = GuardEngine::evaluate(&Guard::AgentVersionCreated, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_patch_applies_cleanly_passes() {
    use verdict::guard::GuardEngine;

    let mut ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    ctx.output = Some(StepOutput::new(
        "--- a/src/test.rs\n+++ b/src/test.rs\n@@ -1,1 +1,2 @@".to_string(),
    ));

    let result = GuardEngine::evaluate(&Guard::PatchAppliesCleanly, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_patch_applies_cleanly_fails_on_non_diff() {
    use verdict::guard::GuardEngine;

    let mut ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    ctx.output = Some(StepOutput::new(
        "this is not a diff".to_string(),
    ));

    let result = GuardEngine::evaluate(&Guard::PatchAppliesCleanly, &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_guard_reflection_has_finding_passes() {
    use verdict::guard::GuardEngine;

    let mut ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    ctx.output = Some(StepOutput::new(
        "Finding: the agent could be improved by...".to_string(),
    ));

    let result = GuardEngine::evaluate(&Guard::ReflectionHasActionableFinding, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_reflection_has_finding_fails() {
    use verdict::guard::GuardEngine;

    let mut ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    ctx.output = Some(StepOutput::new(
        "The agent is already optimal".to_string(),
    ));

    let result = GuardEngine::evaluate(&Guard::ReflectionHasActionableFinding, &ctx).await;
    assert!(result.is_err());
}

// ============================================================================
// Audit Phase 8 Tests
// ============================================================================

#[test]
fn test_audit_event_self_update_proposed() {
    let event = AuditEvent::SelfUpdateProposed {
        agent_name: "test_agent".to_string(),
        risk_level: "Low".to_string(),
    };

    let log = AuditLog::new();
    let mut log = log;
    log.append(AuditEntry {
        timestamp: chrono::Utc::now(),
        pipeline_name: "test".to_string(),
        step_name: "propose".to_string(),
        event,
    });

    let json = log.to_json().unwrap();
    assert!(json.contains("SelfUpdateProposed"));
    assert!(json.contains("test_agent"));
    assert!(json.contains("Low"));
}

#[test]
fn test_audit_event_agent_version_created() {
    let event = AuditEvent::AgentVersionCreated {
        agent_name: "test_agent".to_string(),
        version: "20240101120000".to_string(),
    };

    let mut log = AuditLog::new();
    log.append(AuditEntry {
        timestamp: chrono::Utc::now(),
        pipeline_name: "test".to_string(),
        step_name: "version".to_string(),
        event,
    });

    let json = log.to_json().unwrap();
    assert!(json.contains("AgentVersionCreated"));
    assert!(json.contains("test_agent"));
    assert!(json.contains("20240101120000"));
}

#[test]
fn test_audit_roundtrip_self_update_proposed() {
    let mut log = AuditLog::new();
    log.append(AuditEntry {
        timestamp: chrono::Utc::now(),
        pipeline_name: "test".to_string(),
        step_name: "propose".to_string(),
        event: AuditEvent::SelfUpdateProposed {
            agent_name: "test".to_string(),
            risk_level: "Medium".to_string(),
        },
    });

    let json_str = log.to_json().unwrap();

    // This is a simplified test - in practice you'd write to disk and read back
    assert!(json_str.contains("SelfUpdateProposed"));
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_self_update_result_construction() {
    let result = SelfUpdateResult {
        applied: true,
        new_version: Some(AgentVersion {
            agent_name: "test".to_string(),
            version: "20240101120001".to_string(),
            parent_version: Some("20240101120000".to_string()),
            created_at: chrono::Utc::now(),
            change_summary: "improved".to_string(),
            git_commit: None,
            evaluation_score: Some(0.9),
        }),
        reason: None,
        eval_score: Some(0.9),
    };

    assert!(result.applied);
    assert!(result.new_version.is_some());
    assert!(result.reason.is_none());
    assert_eq!(result.eval_score, Some(0.9));
}

#[test]
fn test_evaluation_expected_variants() {
    let exact = EvaluationExpected::Exact(json!({"test": "value"}));
    let schema = EvaluationExpected::Schema(json!({"type": "object"}));
    let guard = EvaluationExpected::Guard(Guard::ValidJson);

    // Just test that they can be created without panicking
    match exact {
        EvaluationExpected::Exact(_) => (),
        _ => panic!(),
    }

    match schema {
        EvaluationExpected::Schema(_) => (),
        _ => panic!(),
    }

    match guard {
        EvaluationExpected::Guard(_) => (),
        _ => panic!(),
    }
}

#[test]
fn test_risk_level_display() {
    assert_eq!(RiskLevel::Low.to_string(), "Low");
    assert_eq!(RiskLevel::Medium.to_string(), "Medium");
    assert_eq!(RiskLevel::High.to_string(), "High");
    assert_eq!(RiskLevel::Critical.to_string(), "Critical");
}

#[test]
fn test_context_has_trace() {
    let mut ctx = StepContext::new(
        "test".to_string(),
        "test".to_string(),
        "test".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );

    assert!(!ctx.has_trace());

    ctx.trace.append(TraceEntry {
        step_name: "test".to_string(),
        status: "passed".to_string(),
        timestamp: chrono::Utc::now(),
    });

    assert!(ctx.has_trace());
}
