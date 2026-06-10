use thiserror::Error;

use crate::action::ProviderSpec;
use crate::guard::{Guard, GuardError, GuardEngine};
use crate::context::StepContext;

/// A verdict: the decision on whether to allow a step to proceed
#[derive(Clone)]
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

    /// Delegate approval to a second LLM model acting as a judge.
    /// The judge model receives the step output and request, and must produce
    /// a response containing `pass_on_pattern` to approve the step.
    LlmJudge {
        /// System prompt for the judge model
        system: String,
        /// Template for the user message. Use `{output}` and `{request}` as placeholders.
        input_template: String,
        /// Optional override model; uses runner's default LLM client if None
        model: Option<ProviderSpec>,
        /// Substring the judge response must contain to approve the step
        pass_on_pattern: String,
    },

    /// ALL sub-verdicts must pass
    AllOf(Vec<Verdict>),

    /// ANY sub-verdict must pass
    AnyOf(Vec<Verdict>),
}

impl std::fmt::Debug for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Verdict::None => write!(f, "None"),
            Verdict::Automated(g) => f.debug_tuple("Automated").field(g).finish(),
            Verdict::UserApproval { prompt, show_diff } => f
                .debug_struct("UserApproval")
                .field("prompt", prompt)
                .field("show_diff", show_diff)
                .finish(),
            Verdict::LlmJudge { system: _, input_template: _, model, pass_on_pattern } => f
                .debug_struct("LlmJudge")
                .field("model", model)
                .field("pass_on_pattern", pass_on_pattern)
                .finish(),
            Verdict::AllOf(vs) => f.debug_tuple("AllOf").field(&format!("[{} verdicts]", vs.len())).finish(),
            Verdict::AnyOf(vs) => f.debug_tuple("AnyOf").field(&format!("[{} verdicts]", vs.len())).finish(),
        }
    }
}

/// Error from evaluating a verdict
#[derive(Error, Debug)]
pub enum VerdictError {
    #[error("guard failed: {0}")]
    GuardFailed(#[from] GuardError),

    #[error("user approval required: {prompt}")]
    UserApprovalRequired { prompt: &'static str },

    #[error("user approval denied: {prompt}")]
    UserApprovalDenied { prompt: &'static str },

    #[error("I/O error: {0}")]
    IoError(String),

    #[error("all-of verdict failed: {reason}")]
    AllOfFailed { reason: String },

    #[error("any-of verdict failed: all sub-verdicts failed")]
    AnyOfFailed,

    #[error("LlmJudge verdict requires an LLM client but none was configured")]
    NoLlmClient,

    #[error("LlmJudge evaluation failed: {reason}")]
    LlmJudgeFailed { reason: String },

    #[error("LlmJudge pattern error: {0}")]
    BadPattern(String),
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

            Verdict::UserApproval { prompt, show_diff } => {
                use std::io::{self, Write};
                
                if *show_diff {
                    if let Some(output) = &ctx.output {
                        eprintln!("\n--- Output / Diff ---\n{}\n---\n", output.raw);
                    }
                }
                
                eprint!("{} [y/N]: ", prompt);
                io::stderr().flush().map_err(|e| VerdictError::IoError(e.to_string()))?;
                
                let mut line = String::new();
                io::stdin()
                    .read_line(&mut line)
                    .map_err(|e| VerdictError::IoError(e.to_string()))?;
                
                match line.trim().to_lowercase().as_str() {
                    "y" | "yes" => Ok(()),
                    _ => Err(VerdictError::UserApprovalDenied { prompt }),
                }
            }

            Verdict::LlmJudge { system, input_template, model: _, pass_on_pattern } => {
                let llm_client = ctx.llm_client.as_ref()
                    .ok_or(VerdictError::NoLlmClient)?;

                // Render template: replace {output} and {request} placeholders
                let output_str = ctx.output.as_ref().map(|o| o.raw.as_str()).unwrap_or("");
                let request_str = ctx.request.as_str().unwrap_or("");
                let user = input_template
                    .replace("{output}", output_str)
                    .replace("{request}", request_str);

                let req = crate::llm::LlmRequest {
                    system: system.clone(),
                    user,
                    model: "gpt-4o".to_string(),
                    max_tokens: Some(256),
                    history: None,
                    temperature: None,
                };

                let response = llm_client.complete(req).await
                    .map_err(|e| VerdictError::LlmJudgeFailed { reason: e.to_string() })?;

                // Pattern matching: substring check (no regex crate available)
                if response.content.contains(pass_on_pattern.as_str()) {
                    Ok(())
                } else {
                    Err(VerdictError::LlmJudgeFailed {
                        reason: format!(
                            "judge response did not match pattern `{}`: {}",
                            pass_on_pattern, response.content
                        ),
                    })
                }
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
