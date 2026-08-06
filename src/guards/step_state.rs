use super::GuardError;
use crate::context::StepContext;
use serde_json::Value;

pub fn check_step_passed(
    _guard: &super::Guard,
    ctx: &StepContext,
    step_name: &str,
) -> Result<(), GuardError> {
    if let Some(result) = ctx.step_results.get(step_name) {
        if result.verdict_passed {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "StepPassed".to_string(),
                reason: format!("step '{}' did not pass", step_name),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "StepPassed".to_string(),
            reason: format!("step '{}' not found in results", step_name),
        })
    }
}

pub fn check_step_failed(
    _guard: &super::Guard,
    ctx: &StepContext,
    step_name: &str,
) -> Result<(), GuardError> {
    if let Some(result) = ctx.step_results.get(step_name) {
        if !result.verdict_passed {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "StepFailed".to_string(),
                reason: format!("step '{}' passed (expected failure)", step_name),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "StepFailed".to_string(),
            reason: format!("step '{}' not found in results", step_name),
        })
    }
}

pub fn check_user_approved(
    _guard: &super::Guard,
    ctx: &StepContext,
    step_name: &str,
) -> Result<(), GuardError> {
    if let Some(result) = ctx.step_results.get(step_name) {
        if result.verdict_passed {
            Ok(())
        } else {
            Err(GuardError::Failed {
                guard: "UserApproved".to_string(),
                reason: format!("user did not approve step '{}'", step_name),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "UserApproved".to_string(),
            reason: format!("step '{}' not found", step_name),
        })
    }
}

pub fn check_previous_step_matches_schema(
    _guard: &super::Guard,
    ctx: &StepContext,
    step_name: &str,
    schema: &Value,
) -> Result<(), GuardError> {
    if let Some(result) = ctx.step_results.get(step_name) {
        if let Ok(json) = serde_json::from_str::<Value>(&result.output.raw) {
            match jsonschema::JSONSchema::compile(schema) {
                Ok(validator) => match validator.validate(&json) {
                    Ok(_) => Ok(()),
                    Err(_) => Err(GuardError::Failed {
                        guard: "PreviousStepMatchesSchema".to_string(),
                        reason: format!("step '{}' output does not match schema", step_name),
                    }),
                },
                Err(e) => Err(GuardError::ParseError(e.to_string())),
            }
        } else {
            Err(GuardError::Failed {
                guard: "PreviousStepMatchesSchema".to_string(),
                reason: format!("step '{}' output is not valid JSON", step_name),
            })
        }
    } else {
        Err(GuardError::Failed {
            guard: "PreviousStepMatchesSchema".to_string(),
            reason: format!("step '{}' not found in results", step_name),
        })
    }
}
