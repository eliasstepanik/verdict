use super::*;
use crate::context::StepContext;
use crate::action::StepOutput;
use crate::agent::FilesystemPolicy;
use serde_json::json;

fn make_context_with_output(raw: String) -> StepContext {
    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: std::path::PathBuf::from("/tmp"),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: crate::agent::WorkspaceIsolation::None,
        },
    );
    ctx.output = Some(StepOutput {
        raw,
        parsed: None,
        eval_result: None,
    });
    ctx
}

// Tests for check_no_secrets_in_diff

#[test]
fn test_check_no_secrets_in_diff_pass_no_secrets() {
    let ctx = make_context_with_output("--- a/file.rs\n+++ b/file.rs\n@@ -1,3 +1,3 @@\n fn main() {}".to_string());
    let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
    assert!(result.is_ok());
}

#[test]
fn test_check_no_secrets_in_diff_fail_openai_key() {
    let ctx = make_context_with_output(
        "--- a/.env\n+++ b/.env\n@@ -1 @@\n+OPENAI_KEY=sk-proj-1234567890abcdefghijklmnop".to_string()
    );
    let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { guard, reason }) = result {
        assert_eq!(guard, "NoSecretsInDiff");
        assert!(reason.contains("secret"));
    }
}

#[test]
fn test_check_no_secrets_in_diff_fail_aws_key() {
    let ctx = make_context_with_output(
        "--- a/config.txt\n+++ b/config.txt\n@@ -1 @@\n+AWS_KEY=AKIAIOSFODNN7EXAMPLE".to_string()
    );
    let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { guard, reason }) = result {
        assert_eq!(guard, "NoSecretsInDiff");
        assert!(reason.contains("secret"));
    }
}

#[test]
fn test_check_no_secrets_in_diff_fail_private_key() {
    let ctx = make_context_with_output(
        "--- a/id_rsa\n+++ b/id_rsa\n@@ -1 @@\n+-----BEGIN PRIVATE KEY-----".to_string()
    );
    let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { guard, reason }) = result {
        assert_eq!(guard, "NoSecretsInDiff");
        assert!(reason.contains("secret"));
    }
}

#[test]
fn test_check_no_secrets_in_diff_pass_no_output() {
    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: std::path::PathBuf::from("/tmp"),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: crate::agent::WorkspaceIsolation::None,
        },
    );
    let result = check_no_secrets_in_diff(&super::super::Guard::None, &ctx);
    assert!(result.is_ok());
}

// Tests for check_no_secret_exfiltration

#[test]
fn test_check_no_secret_exfiltration_pass_no_secrets() {
    let ctx = make_context_with_output("This is a regular output with no sensitive data.".to_string());
    let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
    assert!(result.is_ok());
}

#[test]
fn test_check_no_secret_exfiltration_fail_openai_key() {
    let ctx = make_context_with_output(
        "API Response: sk-proj-thisIsAFakeButWellFormedOpenAIKey12345".to_string()
    );
    let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { guard, reason }) = result {
        assert_eq!(guard, "NoSecretExfiltration");
        assert!(reason.contains("secret"));
    }
}

#[test]
fn test_check_no_secret_exfiltration_fail_aws_key() {
    let ctx = make_context_with_output(
        "Retrieved credential AKIAIOSFODNN7EXAMPLE from config".to_string()
    );
    let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { guard, reason }) = result {
        assert_eq!(guard, "NoSecretExfiltration");
        assert!(reason.contains("secret"));
    }
}

#[test]
fn test_check_no_secret_exfiltration_fail_private_key() {
    let ctx = make_context_with_output(
        "Key data:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA1234567890abcdef".to_string()
    );
    let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { guard, reason }) = result {
        assert_eq!(guard, "NoSecretExfiltration");
        assert!(reason.contains("secret"));
    }
}

#[test]
fn test_check_no_secret_exfiltration_pass_no_output() {
    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: std::path::PathBuf::from("/tmp"),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: crate::agent::WorkspaceIsolation::None,
        },
    );
    let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
    assert!(result.is_ok());
}

#[test]
fn test_check_no_secret_exfiltration_fail_env_var_secret() {
    let ctx = make_context_with_output(
        "DATABASE_PASSWORD=super_secret_password_123".to_string()
    );
    let result = check_no_secret_exfiltration(&super::super::Guard::None, &ctx);
    assert!(result.is_err());
    if let Err(GuardError::Failed { guard, reason }) = result {
        assert_eq!(guard, "NoSecretExfiltration");
        assert!(reason.contains("secret"));
    }
}

#[test]
fn test_shared_helper_ignores_low_medium_risk() {
    // SecretScanner returns Low/Medium risk for some patterns, but our guard only blocks High/Critical
    // This test verifies that the helper doesn't block on low-risk findings
    let ctx = make_context_with_output("[system]".to_string()); // Low-risk injection pattern
    let result = scan_output_for_secrets(&ctx, "TestGuard");
    assert!(result.is_ok(), "Low-risk patterns should not block the guard");
}
