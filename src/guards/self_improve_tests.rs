use super::*;
use crate::action::StepOutput;
use crate::agent::{AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::context::StepResult;
use std::path::PathBuf;

fn make_test_context() -> StepContext {
    let policy = AgentPolicy {
        max_steps: 10,
        max_retries: 3,
        max_delegation_depth: 2,
        max_cost_usd: None,
        max_runtime_seconds: None,
        allow_self_update: true,
        require_approval_for_self_update: false,
        allowed_agents: vec![],
        allowed_tools: crate::toolset::ToolSet::ReadWrite,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy {
            workspace_root: PathBuf::from("/tmp/test"),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: crate::agent::WorkspaceIsolation::None,
        },
    };

    StepContext::new(
        "test_agent".into(),
        "test_pipeline".into(),
        "test_step".into(),
        serde_json::Value::Null,
        policy.filesystem_policy.clone(),
    )
}

#[test]
fn test_evaluation_improves_or_equal_passes_with_improvement() {
    let mut ctx = make_test_context();

    // Set current output with improved score
    ctx.output = Some(StepOutput::new(
        r#"{"score": 0.95}"#.to_string(),
    ));

    // Add prior score to step_results
    ctx.step_results.insert(
        "prior_evaluation_score".into(),
        StepResult {
            step_name: "eval".into(),
            output: StepOutput::new(r#"{"score": 0.85}"#.to_string()),
            verdict_passed: true,
            error: None,
        },
    );

    let guard = crate::guards::Guard::EvaluationImprovesOrEqual;
    assert!(check_evaluation_improves_or_equal(&guard, &ctx).is_ok());
}

#[test]
fn test_evaluation_improves_or_equal_passes_when_equal() {
    let mut ctx = make_test_context();

    ctx.output = Some(StepOutput::new(r#"{"score": 0.85}"#.to_string()));

    ctx.step_results.insert(
        "prior_evaluation_score".into(),
        StepResult {
            step_name: "eval".into(),
            output: StepOutput::new(r#"{"score": 0.85}"#.to_string()),
            verdict_passed: true,
            error: None,
        },
    );

    let guard = crate::guards::Guard::EvaluationImprovesOrEqual;
    assert!(check_evaluation_improves_or_equal(&guard, &ctx).is_ok());
}

#[test]
fn test_evaluation_improves_or_equal_fails_on_regression() {
    let mut ctx = make_test_context();

    ctx.output = Some(StepOutput::new(r#"{"score": 0.75}"#.to_string()));

    ctx.step_results.insert(
        "prior_evaluation_score".into(),
        StepResult {
            step_name: "eval".into(),
            output: StepOutput::new(r#"{"score": 0.85}"#.to_string()),
            verdict_passed: true,
            error: None,
        },
    );

    let guard = crate::guards::Guard::EvaluationImprovesOrEqual;
    let result = check_evaluation_improves_or_equal(&guard, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { reason, .. }) = result {
        assert!(reason.contains("regressed"));
    }
}

#[test]
fn test_evaluation_improves_or_equal_passes_with_no_prior() {
    let mut ctx = make_test_context();

    ctx.output = Some(StepOutput::new(r#"{"score": 0.75}"#.to_string()));

    // No prior score in step_results - should pass trivially
    let guard = crate::guards::Guard::EvaluationImprovesOrEqual;
    assert!(check_evaluation_improves_or_equal(&guard, &ctx).is_ok());
}

#[test]
fn test_evaluation_improves_or_equal_handles_alternative_score_fields() {
    let mut ctx = make_test_context();

    // Test with overall_score field
    ctx.output = Some(StepOutput::new(
        r#"{"overall_score": 0.90}"#.to_string(),
    ));

    let guard = crate::guards::Guard::EvaluationImprovesOrEqual;
    assert!(check_evaluation_improves_or_equal(&guard, &ctx).is_ok());

    // Test with evaluation_score field
    ctx.output = Some(StepOutput::new(
        r#"{"evaluation_score": 0.88}"#.to_string(),
    ));
    assert!(check_evaluation_improves_or_equal(&guard, &ctx).is_ok());
}

#[test]
fn test_agent_version_created_passes_with_version_in_results() {
    let mut ctx = make_test_context();

    ctx.step_results.insert(
        "self_update_version".into(),
        StepResult {
            step_name: "apply_update".into(),
            output: StepOutput::new(
                r#"{"agent_name": "coder", "version": "20260817120000", "created_at": "2026-08-17T12:00:00Z"}"#
                    .to_string(),
            ),
            verdict_passed: true,
            error: None,
        },
    );

    let guard = crate::guards::Guard::AgentVersionCreated;
    assert!(check_agent_version_created(&guard, &ctx).is_ok());
}

#[test]
fn test_agent_version_created_passes_with_version_in_output() {
    let mut ctx = make_test_context();

    ctx.output = Some(StepOutput::new(
        r#"{"agent_name": "debugger", "version": "20260817130000", "created_at": "2026-08-17T13:00:00Z"}"#
            .to_string(),
    ));

    let guard = crate::guards::Guard::AgentVersionCreated;
    assert!(check_agent_version_created(&guard, &ctx).is_ok());
}

#[test]
fn test_agent_version_created_passes_with_new_version_field() {
    let mut ctx = make_test_context();

    ctx.output = Some(StepOutput::new(
        r#"{"new_version": {"agent_name": "reviewer", "version": "20260817140000"}}"#
            .to_string(),
    ));

    let guard = crate::guards::Guard::AgentVersionCreated;
    assert!(check_agent_version_created(&guard, &ctx).is_ok());
}

#[test]
fn test_agent_version_created_fails_without_version() {
    let ctx = make_test_context();

    let guard = crate::guards::Guard::AgentVersionCreated;
    let result = check_agent_version_created(&guard, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { reason, .. }) = result {
        assert!(reason.contains("AgentVersion"));
    }
}

#[test]
fn test_no_uncommitted_critical_changes_passes_on_clean_repo() {
    use std::process::Command;
    
    // Create a temporary git repository
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_path = temp_dir.path();
    
    // Initialize git repo
    Command::new("git")
        .arg("init")
        .current_dir(repo_path)
        .output()
        .expect("failed to init git repo");
    
    // Configure git for commits
    Command::new("git")
        .arg("config")
        .arg("user.email")
        .arg("test@example.com")
        .current_dir(repo_path)
        .output()
        .expect("failed to set git user.email");
    
    Command::new("git")
        .arg("config")
        .arg("user.name")
        .arg("Test User")
        .current_dir(repo_path)
        .output()
        .expect("failed to set git user.name");
    
    // Create a dummy file and commit it to have a clean state
    std::fs::write(repo_path.join("README.md"), "test").expect("failed to write file");
    Command::new("git")
        .arg("add")
        .arg("README.md")
        .current_dir(repo_path)
        .output()
        .expect("failed to add file");
    
    Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg("initial commit")
        .current_dir(repo_path)
        .output()
        .expect("failed to commit");
    
    // Now test: repo is clean, should pass
    let mut ctx = make_test_context();
    ctx.filesystem_policy.workspace_root = repo_path.to_path_buf();
    
    let guard = crate::guards::Guard::NoActiveUncommittedCriticalChanges;
    let result = check_no_active_uncommitted_critical_changes(&guard, &ctx);
    
    // Clean repo should pass
    assert!(result.is_ok(), "Expected clean repo to pass, got: {:?}", result);
}

#[test]
fn test_no_uncommitted_critical_changes_gracefully_handles_non_git_repo() {
    // Create a temporary directory that is NOT a git repo
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let repo_path = temp_dir.path();
    
    // Create a file but do NOT initialize git
    std::fs::write(repo_path.join("README.md"), "test").expect("failed to write file");
    
    let mut ctx = make_test_context();
    ctx.filesystem_policy.workspace_root = repo_path.to_path_buf();
    
    let guard = crate::guards::Guard::NoActiveUncommittedCriticalChanges;
    let result = check_no_active_uncommitted_critical_changes(&guard, &ctx);
    
    // Non-git directory should gracefully pass (per the guard's behavior)
    assert!(result.is_ok(), "Expected non-git repo to pass gracefully, got: {:?}", result);
}
