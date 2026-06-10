use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::action::{StepAction, StepError, StepOutput, IterationFailureMode};
use crate::agent::Agent;
use crate::audit::{AuditEntry, AuditEvent, AuditLog};
use crate::context::{StepContext, StepResult, TraceEntry};
use crate::guard::{GuardEngine, GuardError};
use crate::pipeline::{FailureMode, Pipeline};
use crate::registry::ToolRegistry;
use crate::tools::ToolContext;
use crate::verdict::{VerdictEngine, VerdictError};
use chrono::Utc;
use serde_json::Value;

/// Which phase of the step the guard is evaluated in
#[derive(Debug, Clone)]
pub enum GuardPhase {
    In,
    Out,
}

/// Error from pipeline execution
#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("step '{step}' failed: {error}")]
    StepFailed { step: String, error: StepError },

    #[error("max retries exceeded for step '{step}'")]
    MaxRetriesExceeded { step: String },

    #[error("guard failed at step '{step}' ({phase:?}): {error}")]
    GuardFailed {
        step: String,
        phase: GuardPhase,
        error: GuardError,
    },

    #[error("verdict failed at step '{step}': {error}")]
    VerdictFailed { step: String, error: VerdictError },

    #[error("awaiting approval at step '{step}': {prompt}")]
    AwaitingApproval { step: String, prompt: &'static str },
}

/// Result of running a pipeline
pub struct PipelineResult {
    pub pipeline_name: String,
    pub steps_passed: Vec<String>,
    pub steps_failed: Vec<String>,
    pub step_results: HashMap<String, StepResult>,
    pub audit_log: AuditLog,
    pub success: bool,
}

/// Executor for pipelines with guards, verdicts, and audit logging
pub struct PipelineRunner {
    pub audit_log: AuditLog,
    pub tool_registry: Arc<ToolRegistry>,
}

impl PipelineRunner {
    pub fn new() -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
        }
    }

    pub fn with_tool_registry(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry,
        }
    }

    /// Execute a pipeline with an agent
    pub async fn run(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
    ) -> Result<PipelineResult, PipelineError> {
        // Start pipeline
        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: pipeline.name.clone(),
            step_name: String::new(),
            event: AuditEvent::PipelineStarted,
        });

        let mut ctx = StepContext::new(
            agent.name.clone(),
            pipeline.name.clone(),
            String::new(),
            input.clone(),
            agent.policy.filesystem_policy.clone(),
        );
        ctx.network_policy = agent.policy.network_policy.clone();

        let mut steps_passed = Vec::new();
        let mut steps_failed = Vec::new();

        for step in &pipeline.steps {
            // Step execution loop
            let mut retry_count = 0;
            let max_retries = pipeline.max_retries;
            let mut step_success = false;

            while retry_count <= max_retries && !step_success {
                ctx.step_name = step.name.clone();

                // 1. Build step context
                ctx.input = input.clone();
                ctx.allowed_tools = step.tools.clone();

                // 2. Compute effective tool scope (Phase 1 stub: use step tools directly)
                // 3. Apply injection protection (Phase 1: just log)
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: pipeline.name.clone(),
                    step_name: step.name.clone(),
                    event: AuditEvent::StepStarted,
                });

                // 4. Run guard_in
                if let Err(e) = GuardEngine::evaluate(&step.guard_in, &ctx).await {
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardFailed {
                            guard: format!("{:?}", step.guard_in),
                            reason: e.to_string(),
                        },
                    });

                    return Err(PipelineError::GuardFailed {
                        step: step.name.clone(),
                        phase: GuardPhase::In,
                        error: e,
                    });
                }

                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: pipeline.name.clone(),
                    step_name: step.name.clone(),
                    event: AuditEvent::GuardPassed {
                        guard: format!("{:?}", step.guard_in),
                    },
                });

                // 5. Execute action
                match self.execute_action(&step.action, &mut ctx).await {
                    Ok(output) => {
                        ctx.output = Some(output);
                    }
                    Err(e) => {
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::StepFailed {
                                error: e.to_string(),
                            },
                        });

                        // Handle failure based on pipeline mode
                        match &pipeline.on_failure {
                            FailureMode::Abort => {
                                steps_failed.push(step.name.clone());
                                return Err(PipelineError::StepFailed {
                                    step: step.name.clone(),
                                    error: e,
                                });
                            }
                            FailureMode::Retry => {
                                retry_count += 1;
                                if retry_count > max_retries {
                                    steps_failed.push(step.name.clone());
                                    return Err(PipelineError::MaxRetriesExceeded {
                                        step: step.name.clone(),
                                    });
                                }
                                continue;
                            }
                            FailureMode::Skip => {
                                steps_failed.push(step.name.clone());
                                break;
                            }
                            FailureMode::Fallback(_) => {
                                // Phase 2+: implement fallback
                                steps_failed.push(step.name.clone());
                                return Err(PipelineError::StepFailed {
                                    step: step.name.clone(),
                                    error: e,
                                });
                            }
                        }
                    }
                }

                // 6. Record trace entry
                ctx.trace.append(TraceEntry {
                    step_name: step.name.clone(),
                    status: "executed".to_string(),
                    timestamp: Utc::now(),
                });

                // 7. Run guard_out
                if let Err(e) = GuardEngine::evaluate(&step.guard_out, &ctx).await {
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardFailed {
                            guard: format!("{:?}", step.guard_out),
                            reason: e.to_string(),
                        },
                    });

                    return Err(PipelineError::GuardFailed {
                        step: step.name.clone(),
                        phase: GuardPhase::Out,
                        error: e,
                    });
                }

                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: pipeline.name.clone(),
                    step_name: step.name.clone(),
                    event: AuditEvent::GuardPassed {
                        guard: format!("{:?}", step.guard_out),
                    },
                });

                // 8. Run verdict
                match VerdictEngine::evaluate(&step.verdict, &ctx).await {
                    Ok(_) => {
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::VerdictPassed {
                                verdict: format!("{:?}", step.verdict),
                            },
                        });

                        // 9. Commit output
                        let result = StepResult {
                            step_name: step.name.clone(),
                            output: ctx.output.clone().unwrap_or_else(|| {
                                StepOutput::new("(no output)".to_string())
                            }),
                            verdict_passed: true,
                            error: None,
                        };
                        ctx.step_results.insert(step.name.clone(), result);

                        steps_passed.push(step.name.clone());
                        step_success = true;

                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::StepCompleted {
                                verdict_passed: true,
                            },
                        });
                    }
                    Err(VerdictError::UserApprovalRequired { prompt }) => {
                        return Err(PipelineError::AwaitingApproval {
                            step: step.name.clone(),
                            prompt,
                        });
                    }
                    Err(e) => {
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::VerdictFailed {
                                verdict: format!("{:?}", step.verdict),
                                reason: e.to_string(),
                            },
                        });

                        // 10. Handle failure
                        match &pipeline.on_failure {
                            FailureMode::Abort => {
                                steps_failed.push(step.name.clone());
                                return Err(PipelineError::VerdictFailed {
                                    step: step.name.clone(),
                                    error: e,
                                });
                            }
                            FailureMode::Retry => {
                                retry_count += 1;
                                if retry_count > max_retries {
                                    steps_failed.push(step.name.clone());
                                    return Err(PipelineError::MaxRetriesExceeded {
                                        step: step.name.clone(),
                                    });
                                }
                                continue;
                            }
                            FailureMode::Skip => {
                                steps_failed.push(step.name.clone());
                                self.audit_log.append(AuditEntry {
                                    timestamp: Utc::now(),
                                    pipeline_name: pipeline.name.clone(),
                                    step_name: step.name.clone(),
                                    event: AuditEvent::StepCompleted {
                                        verdict_passed: false,
                                    },
                                });
                                break;
                            }
                            FailureMode::Fallback(_) => {
                                // Phase 2+: implement fallback
                                steps_failed.push(step.name.clone());
                                return Err(PipelineError::VerdictFailed {
                                    step: step.name.clone(),
                                    error: e,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Finalize
        let success = steps_failed.is_empty();
        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: pipeline.name.clone(),
            step_name: String::new(),
            event: if success {
                AuditEvent::PipelineCompleted {
                    steps_passed: steps_passed.len() as u32,
                    steps_failed: steps_failed.len() as u32,
                }
            } else {
                AuditEvent::PipelineFailed {
                    reason: format!("Failed steps: {:?}", steps_failed),
                }
            },
        });

        Ok(PipelineResult {
            pipeline_name: pipeline.name.clone(),
            steps_passed,
            steps_failed,
            step_results: ctx.step_results,
            audit_log: self.audit_log.clone(),
            success,
        })
    }

    /// Execute a single step action
    async fn execute_action(
        &self,
        action: &StepAction,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        match action {
            StepAction::LlmCall { system: _, user: _, model: _ } => {
                // Phase 1 stub: return static response
                Ok(StepOutput::new(
                    "[LLM stub: no provider configured in Phase 1]".to_string(),
                ))
            }

            StepAction::ToolCall { tool, args } => {
                self.execute_tool_call(tool, args, ctx).await
            }

            StepAction::Custom(f) => f(ctx),

            StepAction::UserInput { prompt, schema: _ } => {
                // Read from stdin
                print!("{} ", prompt);
                io::stdout().flush().ok();

                let mut input = String::new();
                let stdin = io::stdin();
                let mut handle = stdin.lock();
                handle.read_line(&mut input).ok();
                Ok(StepOutput::new(input.trim().to_string()))
            }

            StepAction::DelegateAgent { .. } => {
                Err(StepError::NotImplemented(
                    "DelegateAgent — Phase 4".to_string(),
                ))
            }

            StepAction::SubPipeline(pipeline) => {
                // Recursively execute sub-pipeline with Box::pin to handle async recursion
                let agent = crate::agent::Agent {
                    name: ctx.agent_name.clone(),
                    description: String::new(),
                    pipeline: pipeline.as_ref().clone(),
                    tools: ctx.allowed_tools.clone(),
                    skills: Default::default(),
                    policy: Default::default(),
                };

                let mut runner = PipelineRunner::new();
                let run_future = runner.run(pipeline, &agent, ctx.input.clone());
                match Pin::from(Box::new(run_future)).await {
                    Ok(result) => {
                        // Merge result into context
                        ctx.step_results.extend(result.step_results);
                        ctx.trace.entries.extend(result.audit_log.entries().iter().map(|e| {
                            crate::context::TraceEntry {
                                step_name: e.step_name.clone(),
                                status: format!("{:?}", e.event),
                                timestamp: e.timestamp,
                            }
                        }));

                        Ok(StepOutput::new(
                            format!("SubPipeline completed: {}", result.success),
                        ))
                    }
                    Err(e) => Err(StepError::ActionFailed {
                        reason: format!("SubPipeline failed: {}", e),
                    }),
                }
            }

            StepAction::LoopUntil {
                body,
                condition,
                max_iterations,
                on_iteration_failure,
            } => {
                let mut iteration = 0;
                loop {
                    if iteration >= *max_iterations {
                        break;
                    }

                    // Execute body with Box::pin for async recursion
                    let body_future = self.execute_action(body, ctx);
                    match Pin::from(Box::new(body_future)).await {
                        Ok(output) => {
                            ctx.output = Some(output);
                        }
                        Err(e) => {
                            match on_iteration_failure {
                                IterationFailureMode::Retry => {
                                    iteration += 1;
                                    continue;
                                }
                                IterationFailureMode::Skip => {
                                    iteration += 1;
                                    continue;
                                }
                                IterationFailureMode::Abort => {
                                    return Err(e);
                                }
                            }
                        }
                    }

                    // Check condition
                    match GuardEngine::evaluate(condition, ctx).await {
                        Ok(_) => {
                            // Condition passed, exit loop
                            return Ok(StepOutput::new(format!(
                                "Loop exited after {} iterations",
                                iteration + 1
                            )));
                        }
                        Err(_) => {
                            // Condition failed, continue loop
                            iteration += 1;
                        }
                    }
                }

                Ok(StepOutput::new(format!(
                    "Loop completed (max iterations: {})",
                    max_iterations
                )))
            }

            StepAction::UseSkill { .. } => Err(StepError::NotImplemented(
                "UseSkill — Phase 5".to_string(),
            )),
        }
    }

    /// Execute a tool call with full 8-step protocol
    async fn execute_tool_call(
        &self,
        tool_name: &str,
        args: &Value,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        let audit_log = Arc::new(Mutex::new(self.audit_log.clone()));

        // Step 1: Check tool is registered
        let tool = self
            .tool_registry
            .get(tool_name)
            .ok_or_else(|| StepError::ActionFailed {
                reason: format!("tool '{}' not found in registry", tool_name),
            })?;

        // Step 2: Check tool is allowed for this step
        if !ctx.allowed_tools.contains(tool_name) {
            return Err(StepError::ActionFailed {
                reason: format!(
                    "tool '{}' not allowed in this step (allowed: {:?})",
                    tool_name, ctx.allowed_tools
                ),
            });
        }

        // Step 3: Validate args against tool schema
        let schema = tool.schema();
        if let Ok(validator) = jsonschema::JSONSchema::compile(&schema) {
            if let Err(e) = validator.validate(args) {
                let mut error_msgs = Vec::new();
                for error in e {
                    error_msgs.push(error.to_string());
                }
                return Err(StepError::ActionFailed {
                    reason: format!("schema validation failed: {}", error_msgs.join("; ")),
                });
            }
        }

        // Step 4: Apply tool-specific guards (stub for Phase 2)

        // Step 5: Record audit log — tool call started
        let audit_log_mutex = audit_log.clone();
        audit_log_mutex.lock().ok().map(|mut log| {
            log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::ToolCallStarted {
                    tool: tool_name.to_string(),
                    args: args.to_string(),
                },
            });
        });

        // Step 6: Run tool
        let tool_context = ToolContext {
            filesystem_policy: ctx.filesystem_policy.clone(),
            network_policy: ctx.network_policy.clone(),
            allowed_tools: ctx.allowed_tools.clone(),
            audit_log: audit_log.clone(),
        };

        let tool_result = tool.call(args.clone(), tool_context).await;

        // Step 7: Handle result and record audit log
        match tool_result {
            Ok(output) => {
                let output_bytes = output.raw.len();

                // Record successful tool call
                audit_log_mutex.lock().ok().map(|mut log| {
                    log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: ctx.pipeline_name.clone(),
                        step_name: ctx.step_name.clone(),
                        event: AuditEvent::ToolCallCompleted {
                            tool: tool_name.to_string(),
                            output_bytes,
                        },
                    });
                });

                // Step 8: Sanitize output (stub — pass through)
                // Step 9: Validate output schema (stub — pass through)

                Ok(StepOutput::new(output.raw))
            }
            Err(e) => {
                // Record failed tool call
                audit_log_mutex.lock().ok().map(|mut log| {
                    log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: ctx.pipeline_name.clone(),
                        step_name: ctx.step_name.clone(),
                        event: AuditEvent::ToolCallFailed {
                            tool: tool_name.to_string(),
                            reason: e.to_string(),
                        },
                    });
                });

                Err(StepError::ActionFailed {
                    reason: format!("tool '{}' execution failed: {}", tool_name, e),
                })
            }
        }
    }
}

impl Default for PipelineRunner {
    fn default() -> Self {
        Self::new()
    }
}
