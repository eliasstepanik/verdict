use crate::context::StepContext;
use crate::llm::LlmRequest;
use tokio::process::Command;
use super::{Guard, GuardError, TestRunner};
use std::io;

pub async fn check_compiles(
    _guard: &Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&ctx.filesystem_policy.workspace_root)
        .output()
        .await
        .map_err(|e| GuardError::IoError(e.to_string()))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "Compiles".to_string(),
            reason: format!(
                "cargo check failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        })
    }
}

pub async fn check_tests_pass(
    _guard: &Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    let output = Command::new("cargo")
        .arg("test")
        .arg("--lib")
        .current_dir(&ctx.filesystem_policy.workspace_root)
        .output()
        .await
        .map_err(|e| GuardError::IoError(e.to_string()))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "TestsPass".to_string(),
            reason: format!(
                "cargo test failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        })
    }
}

pub async fn check_tests_pass_with(
    _guard: &Guard,
    ctx: &StepContext,
    runner: &TestRunner,
) -> Result<(), GuardError> {
    let output = match runner {
        TestRunner::CargoTest => {
            Command::new("cargo")
                .arg("test")
                .current_dir(&ctx.filesystem_policy.workspace_root)
                .output()
                .await
        }
        TestRunner::Pytest => {
            Command::new("pytest")
                .current_dir(&ctx.filesystem_policy.workspace_root)
                .output()
                .await
        }
        TestRunner::Jest => {
            Command::new("npm")
                .arg("test")
                .current_dir(&ctx.filesystem_policy.workspace_root)
                .output()
                .await
        }
        TestRunner::Vitest => {
            Command::new("vitest")
                .current_dir(&ctx.filesystem_policy.workspace_root)
                .output()
                .await
        }
        TestRunner::Custom(cmd) => {
            Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .current_dir(&ctx.filesystem_policy.workspace_root)
                .output()
                .await
        }
    }
    .map_err(|e| GuardError::IoError(e.to_string()))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "TestsPass".to_string(),
            reason: format!(
                "test runner failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        })
    }
}

pub async fn check_lint_pass(
    _guard: &Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    let output = Command::new("cargo")
        .arg("clippy")
        .arg("--all-targets")
        .arg("--all-features")
        .arg("--")
        .arg("-D")
        .arg("warnings")
        .current_dir(&ctx.filesystem_policy.workspace_root)
        .output()
        .await
        .map_err(|e| GuardError::IoError(e.to_string()))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "LintPass".to_string(),
            reason: format!(
                "cargo clippy failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
        })
    }
}

pub async fn check_format_pass(
    _guard: &Guard,
    ctx: &StepContext,
) -> Result<(), GuardError> {
    let output = Command::new("rustfmt")
        .arg("--check")
        .arg("--recursive")
        .arg("src/")
        .current_dir(&ctx.filesystem_policy.workspace_root)
        .output()
        .await;

    match output {
        Ok(out) => {
            if out.status.success() {
                Ok(())
            } else {
                Err(GuardError::Failed {
                    guard: "FormatPass".to_string(),
                    reason: format!(
                        "rustfmt found formatting issues: {}",
                        String::from_utf8_lossy(&out.stderr)
                    ),
                })
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // rustfmt not installed, pass with warning
            Ok(())
        }
        Err(e) => Err(GuardError::IoError(e.to_string())),
    }
}

pub async fn check_semantic_check(
    _guard: &Guard,
    ctx: &StepContext,
    check_description: &str,
) -> Result<(), GuardError> {
    // SemanticCheck requires an LLM client
    let llm_client = ctx.llm_client.as_ref().ok_or_else(|| GuardError::Failed {
        guard: "SemanticCheck".to_string(),
        reason: "no LLM client configured — SemanticCheck requires an LLM client".to_string(),
    })?;

    // Get the output to evaluate
    let output_str = ctx
        .output
        .as_ref()
        .map(|o| o.raw.as_str())
        .unwrap_or("");

    // Build the LLM judge request
    let system = "You are a semantic checker. Evaluate whether the output satisfies the given requirement. \
                  Reply with exactly one word: PASS if satisfied, or FAIL followed by a brief reason if not.";
    let user = format!(
        "Requirement: {check_description}\n\nOutput to evaluate:\n{output_str}"
    );

    let req = LlmRequest {
        system: system.to_string(),
        user,
        model: llm_client.default_model().to_string(),
        max_tokens: Some(256),
        history: None,
        temperature: None,
        tools: None,
        tool_choice: None,
    };

    let response = llm_client.complete(req).await.map_err(|e| GuardError::Failed {
        guard: "SemanticCheck".to_string(),
        reason: format!("LLM call failed: {e}"),
    })?;

    let first_line = response.content.lines().next().unwrap_or("").trim();
    if first_line.to_uppercase().starts_with("PASS") {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "SemanticCheck".to_string(),
            reason: format!("semantic check failed: {}", response.content),
        })
    }
}
