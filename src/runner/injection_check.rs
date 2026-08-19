//! Injection protection enforcement for step outputs.
//!
//! When `InjectionProtection::Strict` is enabled on a step, this module scans
//! the step's output for injection patterns and secrets, blocking the step if
//! any are detected.

use crate::action::StepOutput;
use crate::audit::{AuditEntry, AuditEvent};
use crate::context::StepContext;
use crate::injection::{InjectionScanner, RiskLevel, SecretScanner};
use crate::pipeline::{AgentStep, InjectionProtection};
use crate::action::StepError;
use chrono::Utc;
use super::PipelineRunner;

/// Check a step's output for injection patterns and secrets if protection is enabled.
///
/// # Returns
/// - `Ok(())` if protection is disabled (`None`) or no threats detected
/// - `Err(StepError::ActionFailed)` if a threat is detected in `Strict` mode, with
///   a corresponding `AuditEvent::InjectionDetected` or `::SecretDetected` appended
///   to the audit log
///
/// # Audit Logging
/// This function is responsible for appending injection/secret detection events
/// to the audit log when violations occur. Calling code should NOT separately
/// log these events.
pub(crate) async fn check_injection_protection(
    runner: &mut PipelineRunner,
    step: &AgentStep,
    ctx: &StepContext,
    output: &StepOutput,
) -> Result<(), StepError> {
    // If protection is disabled, allow all output through without scanning
    if step.injection_protection == InjectionProtection::None {
        return Ok(());
    }

    // If protection is Strict, scan the output text
    let text = &output.raw;

    // Check for injection patterns first
    let injection_result = InjectionScanner::scan(text);
    if injection_result.detected {
        let pattern = injection_result
            .pattern
            .unwrap_or_else(|| "unknown".to_string());
        let risk_level_str = injection_result
            .risk_level
            .map(|r| format!("{}", r))
            .unwrap_or_else(|| "unknown".to_string());

        // Log the detection
        runner.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: ctx.pipeline_name.clone(),
            step_name: step.name.clone(),
            event: AuditEvent::InjectionDetected {
                pattern: pattern.clone(),
                risk_level: risk_level_str,
            },
        });

        // Block the step
        return Err(StepError::ActionFailed {
            reason: format!(
                "injection pattern detected in step output: {} ({})",
                pattern, injection_result.risk_level.map(|r| format!("{}", r)).unwrap_or_else(|| "unknown".to_string())
            ),
        });
    }

    // Check for secret patterns
    let secret_matches = SecretScanner::scan(text);
    for secret_match in secret_matches {
        // Block on High and Critical risk levels
        if let Some(risk) = secret_match.risk_level {
            if risk == RiskLevel::High || risk == RiskLevel::Critical {
                // Log the detection
                runner.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: ctx.pipeline_name.clone(),
                    step_name: step.name.clone(),
                    event: AuditEvent::SecretDetected {
                        pattern_name: secret_match.pattern_name.clone(),
                    },
                });

                // Block the step
                return Err(StepError::ActionFailed {
                    reason: format!(
                        "secret pattern detected in step output: {} ({})",
                        secret_match.pattern_name, risk
                    ),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
#[path = "injection_check_tests.rs"]
mod tests;
