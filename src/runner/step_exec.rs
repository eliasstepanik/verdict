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
use crate::guards::GuardEngine;
use crate::pipeline::AgentStep;
use crate::verdict::VerdictEngine;
use super::PipelineRunner;
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

/// Run the post-action phase for a step: guard_out, verdict, and `StepCompleted`.
///
/// `output` is published to `ctx.output` first so guards and verdicts observe it.
pub(crate) async fn run_post_action(
    runner: &mut PipelineRunner,
    step: &AgentStep,
    ctx: &mut StepContext,
    output: StepOutput,
) -> Result<StepOutput, StepError> {
    ctx.output = Some(output.clone());

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
            return Err(StepError::ActionFailed {
                reason: format!("guard_out failed: {}", err_str),
            });
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
            return Err(StepError::ActionFailed {
                reason: format!("verdict failed: {}", e),
            });
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
