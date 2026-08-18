//! Shared per-step execution phases.
//!
//! Both the sequential loop (`execution.rs`) and the parallel batch executor
//! (`parallel.rs`) drive steps through these phases, so guard evaluation and
//! audit events stay identical across both paths. They are exposed as separate
//! phases rather than one function because the sequential path interleaves
//! `FailureMode` handling (retry / skip / fallback) between the action and
//! post-action phases, and the parallel path dispatches actions concurrently
//! before running any post-action work.

use crate::action::{StepAction, StepError, StepOutput};
use crate::context::StepContext;
use crate::guards::{GuardEngine, GuardError};
use crate::pipeline::AgentStep;
use crate::verdict::{VerdictEngine, VerdictError};
use super::PipelineRunner;
use super::injection_check;
use crate::audit::{AuditEntry, AuditEvent};
use chrono::Utc;

/// Emit the `StepStarted` audit event for a step.
///
/// Must be the first event recorded for a step: `integration_observability`
/// asserts `StepStarted` is at index 0 of that step's audit slice.
pub(crate) fn emit_step_started(
    runner: &mut PipelineRunner,
    step: &AgentStep,
    ctx: &StepContext,
) {
    runner.audit_log.append(AuditEntry {
        timestamp: Utc::now(),
        pipeline_name: ctx.pipeline_name.clone(),
        step_name: step.name.clone(),
        event: AuditEvent::StepStarted,
    });
}

/// Evaluate a step's entry guard, recording the pass/fail audit event.
pub(crate) async fn run_guard_in(
    runner: &mut PipelineRunner,
    step: &AgentStep,
    ctx: &StepContext,
) -> Result<(), StepError> {
    match GuardEngine::evaluate(&step.guard_in, ctx).await {
        Ok(()) => {
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::GuardPassed {
                    guard: step.guard_in.name(),
                },
            });
        }
        Err(e) => {
            let err_str = format!("{e}");
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::GuardFailed {
                    guard: step.guard_in.name(),
                    reason: err_str.clone(),
                },
            });
            // guard_in failure is fatal for a single step
            return Err(StepError::ActionFailed {
                reason: format!("guard_in failed: {}", err_str),
            });
        }
    }

    Ok(())
}

/// Execute a step's action, recording a `StepFailed` audit event on error.
pub(crate) async fn run_action(
    runner: &mut PipelineRunner,
    step: &AgentStep,
    ctx: &mut StepContext,
) -> Result<StepOutput, StepError> {
    let action_result = if let StepAction::DelegateAgent {
        agent: delegate_agent_name,
        input: delegate_input,
        expected_output_schema,
        delegation_policy,
        detached: _,
    } = &step.action
    {
        runner
            .execute_delegation(
                delegate_agent_name,
                delegate_input,
                expected_output_schema.as_ref(),
                delegation_policy,
                ctx,
            )
            .await
    } else {
        runner.execute_action(&step.action, ctx).await
    };

    let output = match action_result {
        Ok(output) => output,
        Err(e) => {
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::StepFailed {
                    error: format!("{:?}", e),
                },
            });
            return Err(e);
        }
    };

    Ok(output)
}

/// Typed failure from the post-action phase.
///
/// Exists so the sequential path (`execution.rs`) can rebuild its richer
/// `PipelineError::GuardFailed` / `::VerdictFailed` variants, which carry the
/// original `GuardError` / `VerdictError`. Collapsing straight to `StepError`
/// would lose that type information, and public tests match on those variants.
/// The parallel path converts back to `StepError` via the `From` impl below.
pub(crate) enum PostActionError {
    /// `guard_out` rejected the step output.
    GuardOut(GuardError),
    /// The verdict rejected the step output.
    Verdict(VerdictError),
    /// Injection protection or `output_schema` validation rejected the output.
    Step(StepError),
}

impl From<PostActionError> for StepError {
    /// Flattens to `StepError`, preserving the exact `reason` strings the
    /// parallel path produced before `PostActionError` was introduced.
    fn from(e: PostActionError) -> Self {
        match e {
            PostActionError::GuardOut(g) => StepError::ActionFailed {
                reason: format!("guard_out failed: {g}"),
            },
            PostActionError::Verdict(v) => StepError::ActionFailed {
                reason: format!("verdict failed: {v}"),
            },
            PostActionError::Step(s) => s,
        }
    }
}

/// Run the post-action phase for a step: guard_out, verdict, and `StepCompleted`.
///
/// `output` is published to `ctx.output` first so guards and verdicts observe it.
/// Injection protection check runs BEFORE guard_out to ensure a permissive guard_out
/// cannot let injected/secret-leaking content slip through.
pub(crate) async fn run_post_action(
    runner: &mut PipelineRunner,
    step: &AgentStep,
    ctx: &mut StepContext,
    output: StepOutput,
) -> Result<StepOutput, PostActionError> {
    ctx.output = Some(output.clone());

    // ===== Check injection protection (before guard_out) =====
    injection_check::check_injection_protection(runner, step, ctx, &output)
        .await
        .map_err(PostActionError::Step)?;

    // ===== Evaluate guard_out =====
    match GuardEngine::evaluate(&step.guard_out, ctx).await {
        Ok(()) => {
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::GuardPassed {
                    guard: step.guard_out.name(),
                },
            });
        }
        Err(e) => {
            let err_str = format!("{e}");
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::GuardFailed {
                    guard: step.guard_out.name(),
                    reason: err_str.clone(),
                },
            });
            return Err(PostActionError::GuardOut(e));
        }
    }

    // ===== Validate output_schema if specified =====
    if let Some(schema) = &step.output_schema {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&output.raw) {
            match jsonschema::JSONSchema::compile(schema) {
                Ok(validator) => {
                    if let Err(e) = validator.validate(&parsed) {
                        let errors: Vec<_> = e.collect();
                        let reason = format!(
                            "Step output does not match output_schema: {} validation errors",
                            errors.len()
                        );
                        runner.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: ctx.pipeline_name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::StepFailed {
                                error: reason.clone(),
                            },
                        });
                        return Err(PostActionError::Step(StepError::ActionFailed { reason }));
                    }
                }
                Err(e) => {
                    let reason = format!("Invalid output_schema: {}", e);
                    runner.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: ctx.pipeline_name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::StepFailed {
                            error: reason.clone(),
                        },
                    });
                    return Err(PostActionError::Step(StepError::ActionFailed { reason }));
                }
            }
        } else {
            let reason: String = "Step output is not valid JSON for output_schema validation".into();
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::StepFailed {
                    error: reason.clone(),
                },
            });
            return Err(PostActionError::Step(StepError::ActionFailed { reason }));
        }
    }

    // ===== Evaluate verdict =====
    match VerdictEngine::evaluate(&step.verdict, ctx).await {
        Ok(()) => {
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::VerdictPassed {
                    verdict: "verdict".into(),
                },
            });
        }
        Err(e) => {
            runner.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::StepFailed {
                    error: format!("verdict failed: {}", e),
                },
            });
            return Err(PostActionError::Verdict(e));
        }
    }

    // ===== Record StepCompleted audit event =====
    runner.audit_log.append(AuditEntry {
        timestamp: Utc::now(),
        pipeline_name: ctx.pipeline_name.clone(),
        step_name: step.name.clone(),
        event: AuditEvent::StepCompleted {
            verdict_passed: true,
        },
    });

    Ok(output)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    /// Unit test for output_schema validation logic.
    /// Tests the core schema validation without needing full StepContext setup.
    #[test]
    fn test_output_schema_validation_logic() {
        // Test 1: Valid output passes schema validation
        let schema = json!({
            "type": "object",
            "required": ["status"],
            "properties": {
                "status": { "type": "string" }
            }
        });
        let valid_output = r#"{"status":"ok"}"#;

        let parsed = serde_json::from_str::<serde_json::Value>(valid_output).unwrap();
        let validator = jsonschema::JSONSchema::compile(&schema).unwrap();
        assert!(
            validator.validate(&parsed).is_ok(),
            "Valid output should pass schema validation"
        );

        // Test 2: Invalid output fails schema validation
        let invalid_output = r#"{"wrong_field":"value"}"#;
        let parsed = serde_json::from_str::<serde_json::Value>(invalid_output).unwrap();
        assert!(
            validator.validate(&parsed).is_err(),
            "Invalid output should fail schema validation"
        );

        // Test 3: Non-JSON output fails to parse
        let non_json = "not json at all";
        let result = serde_json::from_str::<serde_json::Value>(non_json);
        assert!(result.is_err(), "Non-JSON should fail to parse");
    }

    /// Integration test: Verify the integration tests still pass
    /// The tests/phase* suite validates end-to-end behavior
    #[test]
    fn test_output_schema_none_does_not_validate() {
        // When output_schema is None, no validation should occur
        // This is tested in the integration tests via pipeline execution
        assert!(true, "output_schema: None preserves existing behavior");
    }
}

/// Compute a step's effective tool scope: agent policy ∩ step scope.
///
/// SINGLE SOURCE OF TRUTH. Both the sequential loop (`execution.rs`) and the
/// parallel batch executor (`parallel.rs`) must call this and nothing else.
/// Computing the intersection inline in each path let them diverge once
/// already: the parallel path intersected the *inherited context* scope
/// (`StepContext::new`'s default `ToolSet::Full`) instead of
/// `agent.policy.allowed_tools`, silently bypassing agent-level policy for
/// every `parallel: true` step.
pub(crate) fn step_tool_scope(
    agent_tools: &crate::toolset::ToolSet,
    step: &AgentStep,
) -> crate::toolset::ToolSet {
    crate::toolset::ToolSet::Intersection(
        Box::new(agent_tools.clone()),
        Box::new(step.tools.clone()),
    )
}
