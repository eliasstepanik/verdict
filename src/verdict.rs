use thiserror::Error;

use crate::guard::{Guard, GuardError, GuardEngine};
use crate::context::StepContext;

/// A verdict: the decision on whether to allow a step to proceed
#[derive(Debug, Clone)]
pub enum Verdict {
    /// No verdict required, always pass
    None,

    /// Automated decision based on a guard
    Automated(Guard),

    /// Require user approval with an optional diff display
    UserApproval {
        prompt: &'static str,
        show_diff: bool,
    },

    /// ALL sub-verdicts must pass
    AllOf(Vec<Verdict>),

    /// ANY sub-verdict must pass
    AnyOf(Vec<Verdict>),
}

/// Error from evaluating a verdict
#[derive(Error, Debug)]
pub enum VerdictError {
    #[error("guard failed: {0}")]
    GuardFailed(#[from] GuardError),

    #[error("user approval required: {prompt}")]
    UserApprovalRequired { prompt: &'static str },

    #[error("all-of verdict failed: {reason}")]
    AllOfFailed { reason: String },

    #[error("any-of verdict failed: all sub-verdicts failed")]
    AnyOfFailed,
}

/// Engine for evaluating verdicts
pub struct VerdictEngine;

impl VerdictEngine {
    /// Evaluate a verdict against a step context
    pub async fn evaluate(verdict: &Verdict, ctx: &StepContext) -> Result<(), VerdictError> {
        match verdict {
            Verdict::None => Ok(()),

            Verdict::Automated(guard) => {
                GuardEngine::evaluate(guard, ctx).await?;
                Ok(())
            }

            Verdict::UserApproval { prompt, show_diff: _ } => {
                // In Phase 1, we don't actually prompt the user
                // This is a placeholder that signals user approval is required
                Err(VerdictError::UserApprovalRequired { prompt })
            }

            Verdict::AllOf(verdicts) => {
                for verdict in verdicts {
                    std::pin::Pin::from(Box::new(Self::evaluate(verdict, ctx))).await
                        .map_err(|e| VerdictError::AllOfFailed {
                            reason: e.to_string(),
                        })?;
                }
                Ok(())
            }

            Verdict::AnyOf(verdicts) => {
                for verdict in verdicts {
                    if std::pin::Pin::from(Box::new(Self::evaluate(verdict, ctx)))
                        .await
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
                Err(VerdictError::AnyOfFailed)
            }
        }
    }
}
