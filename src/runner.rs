use async_recursion::async_recursion;
use futures::stream::StreamExt;
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex as TokioMutex;

/// Resolve template placeholders in a prompt string.
///
/// Supported placeholders:
/// - `{input}` → the pipeline input value (as string)
/// - `{step_name}` → the raw output of the named prior step
fn resolve_template(template: &str, ctx: &crate::context::StepContext) -> String {
    let mut result = template.to_string();

    // Substitute {input}
    let input_str = match &ctx.input {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(map) => {
            // If input is {"task": "..."}, extract the "task" field preferentially
            if let Some(serde_json::Value::String(task)) = map.get("task") {
                task.clone()
            } else {
                ctx.input.to_string()
            }
        }
        v => v.to_string(),
    };
    result = result.replace("{input}", &input_str);

    // Substitute {step_name} for each prior step result
    for (step_name, step_result) in &ctx.step_results {
        let placeholder = format!("{{{}}}", step_name);
        result = result.replace(&placeholder, &step_result.output.raw);
    }

    result
}

use crate::action::{
    IterationFailureMode, SkillMode, StepAction, StepError, StepOutput, StopCondition,
};
use crate::agent::Agent;
use crate::audit::{AuditEntry, AuditEvent, AuditLog};
use crate::context::{StepContext, StepResult, TraceEntry};
use crate::guard::{GuardEngine, GuardError};
use crate::pipeline::{FailureMode, Pipeline};
use crate::registry::{AgentRegistry, ToolRegistry};
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

    #[error("delegation failed at step '{step}' (agent '{agent}'): {reason}")]
    DelegationFailed {
        step: String,
        agent: String,
        reason: String,
    },
}

/// Result of running a pipeline
#[derive(Debug, Clone)]
pub struct PipelineResult {
    pub pipeline_name: String,
    pub steps_passed: Vec<String>,
    pub steps_failed: Vec<String>,
    pub step_results: HashMap<String, StepResult>,
    pub audit_log: AuditLog,
    pub success: bool,
}

/// An event emitted to an output sink during pipeline execution
#[derive(Debug, Clone)]
pub enum OutputEvent {
    /// A chunk of LLM output (for streaming)
    LlmChunk { step: String, delta: String },
    /// A tool produced a chunk of output
    ToolChunk {
        step: String,
        tool: String,
        delta: String,
    },
    /// A step completed
    StepCompleted { step: String, output: StepOutput },
    /// The pipeline completed
    PipelineCompleted { result: PipelineResult },
}

/// Trait for receiving pipeline output events (streaming support)
#[async_trait::async_trait]
pub trait OutputSink: Send + Sync {
    /// Emit an output event. Fire-and-forget — caller does not await completion.
    async fn emit(&self, event: OutputEvent);
}

/// Executor for pipelines with guards, verdicts, and audit logging
#[derive(Clone)]
pub struct PipelineRunner {
    pub audit_log: AuditLog,
    pub tool_registry: Arc<ToolRegistry>,
    pub agent_registry: Arc<AgentRegistry>,
    pub skill_registry: Arc<crate::skills::registry::SkillRegistry>,
    pub llm_client: Option<Arc<crate::llm::LlmClient>>,
    pub output_sink: Option<Arc<dyn OutputSink>>,
    pub conversation_registry: Arc<std::sync::Mutex<crate::llm::ConversationRegistry>>,
    pub context_store: Option<std::sync::Arc<crate::context::ContextStore>>,
}

impl PipelineRunner {
    pub fn new() -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            agent_registry: Arc::new(AgentRegistry::new()),
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            context_store: None,
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
        }
    }

    pub fn with_tool_registry(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry,
            agent_registry: Arc::new(AgentRegistry::new()),
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            context_store: None,
        }
    }

    /// Create a runner with an agent registry for delegation support
    pub fn with_agent_registry(agent_registry: Arc<AgentRegistry>) -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            agent_registry,
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            context_store: None,
        }
    }

    /// Create a runner with both tool and agent registries
    pub fn with_registries(
        tool_registry: Arc<ToolRegistry>,
        agent_registry: Arc<AgentRegistry>,
    ) -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry,
            agent_registry,
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            context_store: None,
        }
    }

    /// Create a runner with a skill registry for skill support
    pub fn with_skill_registry(
        skill_registry: Arc<crate::skills::registry::SkillRegistry>,
    ) -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            agent_registry: Arc::new(AgentRegistry::new()),
            skill_registry,
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            context_store: None,
        }
    }

    /// Add an LLM client to this runner
    pub fn with_llm_client(mut self, client: Arc<crate::llm::LlmClient>) -> Self {
        self.llm_client = Some(client);
        self
    }

    /// Add an output sink for streaming events
    pub fn with_output_sink(mut self, sink: Arc<dyn OutputSink>) -> Self {
        self.output_sink = Some(sink);
        self
    }

    /// Add a ContextStore for step context persistence
    pub fn with_context_store(mut self, dir: std::path::PathBuf) -> Self {
        self.context_store = Some(std::sync::Arc::new(crate::context::ContextStore::new(dir)));
        self
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
        ctx.agent_registry = self.agent_registry.clone();
        ctx.tool_registry = self.tool_registry.clone();
        ctx.skill_registry = self.skill_registry.clone();
        ctx.llm_client = self.llm_client.clone();

        let mut steps_passed = Vec::new();
        let mut steps_failed = Vec::new();

        let mut step_idx = 0;
        while step_idx < pipeline.steps.len() {
            let step = &pipeline.steps[step_idx];

            // Check if this is a parallel step — if so, collect all consecutive parallel steps
            if step.parallel {
                // Collect all consecutive parallel steps starting from step_idx
                let mut parallel_batch_indices = vec![step_idx];
                let mut batch_idx = step_idx + 1;
                while batch_idx < pipeline.steps.len() && pipeline.steps[batch_idx].parallel {
                    parallel_batch_indices.push(batch_idx);
                    batch_idx += 1;
                }

                // Execute parallel steps with true tokio::task::spawn concurrency
                use tokio::task::JoinSet;

                let mut join_set: JoinSet<(String, Result<(StepContext, StepOutput), String>)> =
                    JoinSet::new();

                for &idx in &parallel_batch_indices {
                    let step_def = pipeline.steps[idx].clone();
                    let mut local_ctx = ctx.clone();
                    local_ctx.step_name = step_def.name.clone();
                    local_ctx.input = input.clone();
                    local_ctx.allowed_tools = step_def.tools.clone();

                    let ctx_arc = Arc::new(TokioMutex::new(local_ctx));

                    // Clone runner and step info for spawn closure
                    let runner = self.clone();
                    let step_name = step_def.name.clone();
                    let action = step_def.action.clone();
                    let guard_in = step_def.guard_in.clone();
                    let guard_out = step_def.guard_out.clone();
                    let verdict = step_def.verdict.clone();

                    join_set.spawn(async move {
                        // guard_in check
                        {
                            let ctx_guard = ctx_arc.lock().await;
                            if let Err(e) = GuardEngine::evaluate(&guard_in, &*ctx_guard).await {
                                return (step_name, Err(format!("guard_in: {}", e)));
                            }
                        }

                        // Execute action
                        let action_result = runner.execute_action(&action, ctx_arc.clone()).await;

                        match action_result {
                            Ok(output) => {
                                // Lock context to update output and check guard_out
                                let mut ctx_guard = ctx_arc.lock().await;
                                ctx_guard.output = Some(output.clone());

                                // guard_out check
                                if let Err(e) = GuardEngine::evaluate(&guard_out, &*ctx_guard).await
                                {
                                    return (step_name, Err(format!("guard_out: {}", e)));
                                }

                                // verdict check
                                if let Err(e) = VerdictEngine::evaluate(&verdict, &*ctx_guard).await
                                {
                                    return (step_name, Err(format!("verdict: {}", e)));
                                }

                                // Extract final context for return (clone to avoid holding lock)
                                let final_ctx = ctx_guard.clone();
                                drop(ctx_guard);

                                (step_name, Ok((final_ctx, output)))
                            }
                            Err(e) => (step_name, Err(format!("action: {}", e.to_string()))),
                        }
                    });
                }

                // Join all parallel tasks
                let mut any_failed = false;
                while let Some(join_result) = join_set.join_next().await {
                    match join_result {
                        Ok((step_name, Ok((step_ctx, output)))) => {
                            // Merge results (last-writer-wins)
                            let sr = StepResult {
                                step_name: step_name.clone(),
                                output: output.clone(),
                                verdict_passed: true,
                                error: None,
                            };
                            ctx.step_results.insert(step_name.clone(), sr);

                            // Auto-save context after parallel step succeeds
                            if let Some(store) = &self.context_store {
                                if let Err(e) = store.save(&ctx).await {
                                    eprintln!("[verdict] warning: ContextStore::save failed: {e}");
                                }
                            }
                            // Merge trace entries
                            ctx.trace.entries.extend(step_ctx.trace.entries);
                            steps_passed.push(step_name);
                        }
                        Ok((step_name, Err(reason))) => {
                            let sr = StepResult {
                                step_name: step_name.clone(),
                                output: StepOutput::new(String::new()),
                                verdict_passed: false,
                                error: Some(reason),
                            };
                            ctx.step_results.insert(step_name.clone(), sr);
                            steps_failed.push(step_name);
                            any_failed = true;
                        }
                        Err(join_err) => {
                            return Err(PipelineError::StepFailed {
                                step: "parallel_batch".to_string(),
                                error: crate::action::StepError::ActionFailed {
                                    reason: format!("task join error: {}", join_err),
                                },
                            });
                        }
                    }
                }

                if any_failed {
                    match &pipeline.on_failure {
                        FailureMode::Abort => {
                            return Err(PipelineError::StepFailed {
                                step: "parallel_batch".to_string(),
                                error: crate::action::StepError::ActionFailed {
                                    reason: "one or more parallel steps failed".to_string(),
                                },
                            });
                        }
                        FailureMode::Skip => { /* continue */ }
                        FailureMode::Retry => { /* treat as sequential retry is not applicable for parallel */
                        }
                        FailureMode::Fallback(fallback_pipeline) => {
                            let fallback = fallback_pipeline.as_ref().clone();
                            let _ = Box::pin(self.run(&fallback, agent, ctx.input.clone())).await;
                        }
                    }
                }

                step_idx += parallel_batch_indices.len();
                continue;
            }

            // Sequential step execution
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
                // DelegateAgent is handled here (not in execute_action) so we have
                // &mut self and can write delegation audit events to self.audit_log.
                let action_result = if let StepAction::DelegateAgent {
                    agent: ref agent_name,
                    ref input,
                    ref expected_output_schema,
                    ref delegation_policy,
                } = step.action
                {
                    self.execute_delegation(
                        agent_name,
                        input,
                        expected_output_schema.as_ref(),
                        delegation_policy,
                        &mut ctx,
                    )
                    .await
                } else {
                    let ctx_arc = Arc::new(TokioMutex::new(ctx.clone()));
                    let action_result = self.execute_action(&step.action, ctx_arc.clone()).await;
                    // Merge back any changes (e.g., conversation history, budget, trace, etc.)
                    ctx = match Arc::try_unwrap(ctx_arc) {
                        Ok(mutex) => mutex.into_inner(),
                        Err(arc) => {
                            // If there are still references, we need to block to get the value
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async { arc.lock().await }).clone()
                        }
                    };
                    action_result
                };

                match action_result {
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
                            FailureMode::Fallback(fallback_pipeline) => {
                                steps_failed.push(step.name.clone());
                                let fallback = fallback_pipeline.as_ref().clone();

                                self.audit_log.append(AuditEntry {
                                    timestamp: Utc::now(),
                                    pipeline_name: pipeline.name.clone(),
                                    step_name: step.name.clone(),
                                    event: AuditEvent::FallbackTriggered {
                                        step: step.name.clone(),
                                        reason: e.to_string(),
                                    },
                                });

                                let mut fallback_ctx = ctx.clone();
                                fallback_ctx.step_results.clear();

                                match Box::pin(self.run(&fallback, agent, ctx.input.clone())).await
                                {
                                    Ok(_fallback_result) => {
                                        // Fallback succeeded; step failed but pipeline continues
                                        break;
                                    }
                                    Err(_fallback_err) => {
                                        return Err(PipelineError::StepFailed {
                                            step: step.name.clone(),
                                            error: e,
                                        });
                                    }
                                }
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
                            output: ctx
                                .output
                                .clone()
                                .unwrap_or_else(|| StepOutput::new("(no output)".to_string())),
                            verdict_passed: true,
                            error: None,
                        };
                        ctx.step_results.insert(step.name.clone(), result);

                        // Auto-save context to ContextStore if enabled
                        if let Some(store) = &self.context_store {
                            if let Err(e) = store.save(&ctx).await {
                                eprintln!("[verdict] warning: ContextStore::save failed: {e}");
                            }
                        }

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

                        // Emit step completion event to output sink
                        if let Some(sink) = &self.output_sink {
                            sink.emit(OutputEvent::StepCompleted {
                                step: step.name.clone(),
                                output: ctx
                                    .output
                                    .clone()
                                    .unwrap_or_else(|| StepOutput::new("(no output)".to_string())),
                            })
                            .await;
                        }
                    }
                    Err(VerdictError::UserApprovalRequired { prompt }) => {
                        return Err(PipelineError::AwaitingApproval {
                            step: step.name.clone(),
                            prompt,
                        });
                    }
                    Err(VerdictError::UserApprovalDenied { prompt }) => {
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::VerdictFailed {
                                verdict: format!("UserApproval"),
                                reason: format!("user denied: {}", prompt),
                            },
                        });

                        // Handle as verdict failure with the on_failure policy
                        match &pipeline.on_failure {
                            FailureMode::Abort => {
                                steps_failed.push(step.name.clone());
                                return Err(PipelineError::VerdictFailed {
                                    step: step.name.clone(),
                                    error: VerdictError::UserApprovalDenied { prompt },
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

                                // Emit step completion event to output sink
                                if let Some(sink) = &self.output_sink {
                                    sink.emit(OutputEvent::StepCompleted {
                                        step: step.name.clone(),
                                        output: ctx.output.clone().unwrap_or_else(|| {
                                            StepOutput::new("(no output)".to_string())
                                        }),
                                    })
                                    .await;
                                }
                                break;
                            }
                            FailureMode::Fallback(fallback_pipeline) => {
                                steps_failed.push(step.name.clone());
                                let fallback = fallback_pipeline.as_ref().clone();

                                self.audit_log.append(AuditEntry {
                                    timestamp: Utc::now(),
                                    pipeline_name: pipeline.name.clone(),
                                    step_name: step.name.clone(),
                                    event: AuditEvent::FallbackTriggered {
                                        step: step.name.clone(),
                                        reason: format!("user denied approval"),
                                    },
                                });

                                let mut fallback_ctx = ctx.clone();
                                fallback_ctx.step_results.clear();

                                match Box::pin(self.run(&fallback, agent, ctx.input.clone())).await
                                {
                                    Ok(_fallback_result) => {
                                        // Fallback succeeded; step failed but pipeline continues
                                        break;
                                    }
                                    Err(_fallback_err) => {
                                        return Err(PipelineError::VerdictFailed {
                                            step: step.name.clone(),
                                            error: VerdictError::UserApprovalDenied { prompt },
                                        });
                                    }
                                }
                            }
                        }
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

                                // Emit step completion event to output sink
                                if let Some(sink) = &self.output_sink {
                                    sink.emit(OutputEvent::StepCompleted {
                                        step: step.name.clone(),
                                        output: ctx.output.clone().unwrap_or_else(|| {
                                            StepOutput::new("(no output)".to_string())
                                        }),
                                    })
                                    .await;
                                }
                                break;
                            }
                            FailureMode::Fallback(fallback_pipeline) => {
                                steps_failed.push(step.name.clone());
                                let fallback = fallback_pipeline.as_ref().clone();

                                self.audit_log.append(AuditEntry {
                                    timestamp: Utc::now(),
                                    pipeline_name: pipeline.name.clone(),
                                    step_name: step.name.clone(),
                                    event: AuditEvent::FallbackTriggered {
                                        step: step.name.clone(),
                                        reason: e.to_string(),
                                    },
                                });

                                let mut fallback_ctx = ctx.clone();
                                fallback_ctx.step_results.clear();

                                match Box::pin(self.run(&fallback, agent, ctx.input.clone())).await
                                {
                                    Ok(_fallback_result) => {
                                        // Fallback succeeded; step failed but pipeline continues
                                        break;
                                    }
                                    Err(_fallback_err) => {
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
            }

            // Advance to next step in the outer while loop
            step_idx += 1;
        }

        // Finalize: append the completion event BEFORE snapshotting into PipelineResult
        // so the result's audit_log contains the final event.
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
        let pipeline_result = PipelineResult {
            pipeline_name: pipeline.name.clone(),
            steps_passed,
            steps_failed,
            step_results: ctx.step_results,
            audit_log: self.audit_log.clone(),
            success,
        };

        // Emit pipeline completion event to output sink
        if let Some(sink) = &self.output_sink {
            sink.emit(OutputEvent::PipelineCompleted {
                result: pipeline_result.clone(),
            })
            .await;
        }

        Ok(pipeline_result)
    }

    /// Execute a single step action
    #[async_recursion]
    pub async fn execute_action(
        &self,
        action: &StepAction,
        ctx: Arc<TokioMutex<StepContext>>,
    ) -> Result<StepOutput, StepError> {
        match action {
            StepAction::LlmCall {
                system,
                user,
                model,
                conversation_id,
                append_to_history,
            } => {
                let client = self
                    .llm_client
                    .as_ref()
                    .ok_or_else(|| StepError::ActionFailed {
                        reason: "no LLM client configured".into(),
                    })?;

                let model_str = match model {
                    Some(spec) => spec.model.clone(),
                    None => String::new(), // empty → provider uses its configured default
                };

                // Resolve template placeholders in system and user prompts
                let (resolved_system, resolved_user, history) = {
                    let ctx_lock = ctx.lock().await;
                    let sys = resolve_template(system, &ctx_lock);
                    let usr = resolve_template(user, &ctx_lock);
                    // Resolve history: named conversation takes precedence over per-step history
                    let hist = if let Some(conv_id) = conversation_id {
                        let reg = self.conversation_registry.lock().unwrap();
                        reg.get(conv_id).filter(|h| !h.is_empty()).cloned()
                    } else {
                        if !ctx_lock.conversation_history.is_empty() {
                            Some(ctx_lock.conversation_history.clone())
                        } else {
                            None
                        }
                    };
                    (sys, usr, hist)
                };

                let req = crate::llm::LlmRequest {
                    system: resolved_system.clone(),
                    user: resolved_user.clone(),
                    model: model_str,
                    max_tokens: None,
                    history,
                    temperature: None,
                    tools: None,
                };

                let resp = client
                    .complete(req)
                    .await
                    .map_err(|e| StepError::ActionFailed {
                        reason: e.to_string(),
                    })?;

                // Wire budget tracking
                {
                    let mut ctx_lock = ctx.lock().await;
                    ctx_lock.budget.llm_calls_used += 1;
                    if let Some(usage) = &resp.usage {
                        let cost = ((usage.completion_tokens as f64) * 2.0
                            + (usage.prompt_tokens as f64))
                            * 0.000001;
                        if let Some(remaining) = ctx_lock.budget.remaining_usd {
                            ctx_lock.budget.remaining_usd = Some((remaining - cost).max(0.0));
                        }
                    }
                }

                // Append to conversation history (both named registry and per-step)
                if *append_to_history {
                    // Update named conversation if specified
                    if let Some(conv_id) = conversation_id {
                        let mut reg = self.conversation_registry.lock().unwrap();
                        let history = reg.get_or_create(conv_id);
                        history.push(crate::llm::provider::ChatRole::User, resolved_user.clone());
                        history.push(
                            crate::llm::provider::ChatRole::Assistant,
                            resp.content.clone(),
                        );
                    }
                    // Also update per-step conversation history
                    {
                        let mut ctx_lock = ctx.lock().await;
                        ctx_lock
                            .conversation_history
                            .push(crate::llm::provider::ChatRole::User, resolved_user.clone());
                        ctx_lock.conversation_history.push(
                            crate::llm::provider::ChatRole::Assistant,
                            resp.content.clone(),
                        );
                    }
                }

                Ok(StepOutput::new(resp.content))
            }

            StepAction::ToolCall { tool, args } => {
                let mut ctx_guard = ctx.lock().await;
                self.execute_tool_call(tool, args, &mut *ctx_guard).await
            }

            StepAction::Custom(f) => {
                let mut ctx_unwrap = ctx.lock().await;
                f(&mut *ctx_unwrap)
            }

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

            StepAction::DelegateAgent {
                agent: agent_name,
                input,
                expected_output_schema: _expected_output_schema,
                delegation_policy: _delegation_policy,
            } => {
                // For DelegateAgent nested in LoopUntil/SubPipeline:
                // Since we're in execute_action (&self), we can't call execute_delegation (&mut self).
                // Instead, look up the agent and execute its pipeline directly.

                let (child_agent, agent_registry) = {
                    let ctx_lock = ctx.lock().await;
                    let child_agent = ctx_lock.agent_registry.get(agent_name).ok_or_else(|| {
                        StepError::ActionFailed {
                            reason: format!("agent '{}' not found in registry", agent_name),
                        }
                    })?;
                    (child_agent, ctx_lock.agent_registry.clone())
                };

                // Create a new runner with shared registries
                let mut child_runner =
                    PipelineRunner::with_registries(self.tool_registry.clone(), agent_registry);
                child_runner.skill_registry = self.skill_registry.clone();

                // Copy llm_client if present
                if let Some(llm_client) = &self.llm_client {
                    child_runner = child_runner.with_llm_client(llm_client.clone());
                }

                // Execute child agent's pipeline
                let child_input = input.clone();
                let run_future = child_runner.run(&child_agent.pipeline, &child_agent, child_input);
                match Pin::from(Box::new(run_future)).await {
                    Ok(result) => {
                        // Merge results back into context
                        {
                            let mut ctx_lock = ctx.lock().await;
                            ctx_lock.step_results.extend(result.step_results);
                            ctx_lock
                                .trace
                                .entries
                                .extend(result.audit_log.entries().iter().map(|e| {
                                    crate::context::TraceEntry {
                                        step_name: format!("{}.{}", agent_name, e.step_name),
                                        status: format!("{:?}", e.event),
                                        timestamp: e.timestamp,
                                    }
                                }));
                        }

                        Ok(StepOutput::new(format!(
                            "DelegateAgent '{}' completed: {}",
                            agent_name, result.success
                        )))
                    }
                    Err(e) => Err(StepError::ActionFailed {
                        reason: format!("DelegateAgent '{}' failed: {}", agent_name, e),
                    }),
                }
            }

            StepAction::SubPipeline(pipeline) => {
                // Recursively execute sub-pipeline with Box::pin to handle async recursion
                let (agent_name, allowed_tools, input) = {
                    let ctx_lock = ctx.lock().await;
                    (
                        ctx_lock.agent_name.clone(),
                        ctx_lock.allowed_tools.clone(),
                        ctx_lock.input.clone(),
                    )
                };

                let agent = crate::agent::Agent {
                    name: agent_name,
                    description: String::new(),
                    pipeline: pipeline.as_ref().clone(),
                    tools: allowed_tools,
                    skills: Default::default(),
                    policy: Default::default(),
                };

                let mut runner = PipelineRunner::new();
                let run_future = runner.run(pipeline, &agent, input);
                match Pin::from(Box::new(run_future)).await {
                    Ok(result) => {
                        // Merge result into context
                        {
                            let mut ctx_lock = ctx.lock().await;
                            ctx_lock.step_results.extend(result.step_results);
                            ctx_lock
                                .trace
                                .entries
                                .extend(result.audit_log.entries().iter().map(|e| {
                                    crate::context::TraceEntry {
                                        step_name: e.step_name.clone(),
                                        status: format!("{:?}", e.event),
                                        timestamp: e.timestamp,
                                    }
                                }));
                        }

                        Ok(StepOutput::new(format!(
                            "SubPipeline completed: {}",
                            result.success
                        )))
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

                    // Execute body recursively
                    match self.execute_action(body, ctx.clone()).await {
                        Ok(output) => {
                            let mut ctx_lock = ctx.lock().await;
                            ctx_lock.output = Some(output);
                        }
                        Err(e) => match on_iteration_failure {
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
                        },
                    }

                    // Check condition
                    let ctx_lock = ctx.lock().await;
                    match GuardEngine::evaluate(condition, &*ctx_lock).await {
                        Ok(_) => {
                            // Condition passed, exit loop
                            drop(ctx_lock);
                            return Ok(StepOutput::new(format!(
                                "Loop exited after {} iterations",
                                iteration + 1
                            )));
                        }
                        Err(_) => {
                            // Condition failed, continue loop
                            drop(ctx_lock);
                            iteration += 1;
                        }
                    }
                }

                Ok(StepOutput::new(format!(
                    "Loop completed (max iterations: {})",
                    max_iterations
                )))
            }

            StepAction::UseSkill {
                skill,
                input: _skill_input,
                mode,
            } => {
                // Look up the skill in the registry
                let skill_def = {
                    let ctx_lock = ctx.lock().await;
                    match ctx_lock.skill_registry.get(skill) {
                        Some(s) => s,
                        None => {
                            return Err(StepError::ActionFailed {
                                reason: format!("Skill '{}' not found in registry", skill),
                            })
                        }
                    }
                };

                // Track active skill usage
                {
                    let mut ctx_lock = ctx.lock().await;
                    ctx_lock.active_skills.push(skill.clone());
                }

                // Execute based on mode
                match mode {
                    SkillMode::PromptOnly => {
                        // Return the skill's instructions as output
                        Ok(StepOutput::new(skill_def.instructions.clone()))
                    }
                    SkillMode::Pipeline => {
                        // Run skill's pipeline if available, else fall back to instructions
                        if let Some(pipeline) = skill_def.pipeline.clone() {
                            // Execute the sub-pipeline recursively
                            let sub_action = StepAction::SubPipeline(Box::new(pipeline));
                            match self.execute_action(&sub_action, ctx.clone()).await {
                                Ok(output) => Ok(output),
                                Err(e) => Err(e),
                            }
                        } else {
                            Ok(StepOutput::new(skill_def.instructions.clone()))
                        }
                    }
                    SkillMode::Auto => {
                        // If pipeline available, run it; else prompt-only
                        if let Some(pipeline) = skill_def.pipeline.clone() {
                            let sub_action = StepAction::SubPipeline(Box::new(pipeline));
                            match self.execute_action(&sub_action, ctx.clone()).await {
                                Ok(output) => Ok(output),
                                Err(e) => Err(e),
                            }
                        } else {
                            Ok(StepOutput::new(skill_def.instructions.clone()))
                        }
                    }
                }
            }

            StepAction::Branch {
                condition,
                if_true,
                if_false,
            } => {
                // Evaluate condition against previous step output or context
                // Simple string evaluation: check if condition appears in output
                let prev_output = {
                    let ctx_lock = ctx.lock().await;
                    ctx_lock
                        .output
                        .as_ref()
                        .map(|o| o.raw.clone())
                        .unwrap_or_default()
                };
                let condition_matches = prev_output.contains(condition);

                if condition_matches {
                    // Execute if_true
                    self.execute_action(if_true, ctx.clone()).await
                } else if let Some(if_false_action) = if_false {
                    // Execute if_false
                    self.execute_action(if_false_action, ctx.clone()).await
                } else {
                    // No else branch, just return the previous output
                    Ok(StepOutput::new(prev_output))
                }
            }

            StepAction::RemoteAgent {
                endpoint,
                agent_name,
                payload,
            } => {
                let client = crate::agent::RemoteAgentClient::new();
                match client.execute(endpoint, agent_name, payload.clone()).await {
                    Ok(result) => {
                        // Convert result to StepOutput
                        let output =
                            serde_json::to_string(&result).unwrap_or_else(|_| result.to_string());
                        Ok(StepOutput::with_parsed(output, result))
                    }
                    Err(e) => Err(StepError::ActionFailed {
                        reason: format!("remote agent execution failed: {}", e),
                    }),
                }
            }

            StepAction::LlmCallStreaming {
                system,
                user,
                model,
            } => {
                // Stream the LLM response, emitting each chunk via OutputSink as it arrives.
                // Guards and verdicts run on the fully assembled response after streaming completes.
                let client = self
                    .llm_client
                    .as_ref()
                    .ok_or_else(|| StepError::ActionFailed {
                        reason: "no LLM client configured".into(),
                    })?;

                let model_str = match model {
                    Some(spec) => spec.model.clone(),
                    None => String::new(), // empty → provider uses its configured default
                };

                // Resolve template placeholders in system and user prompts
                let (resolved_system, resolved_user, history) = {
                    let ctx_lock = ctx.lock().await;
                    let sys = resolve_template(system, &ctx_lock);
                    let usr = resolve_template(user, &ctx_lock);
                    let hist = if ctx_lock.conversation_history.is_empty() {
                        None
                    } else {
                        Some(ctx_lock.conversation_history.clone())
                    };
                    (sys, usr, hist)
                };

                let req = crate::llm::LlmRequest {
                    system: resolved_system.clone(),
                    user: resolved_user.clone(),
                    model: model_str,
                    max_tokens: None,
                    history,
                    temperature: None,
                    tools: None,
                };

                // Call the provider's stream method to get chunks
                let mut stream = client.stream(req);
                let mut full_response = String::new();

                // Consume the stream, emitting each chunk to the sink
                while let Some(chunk_result) = stream.next().await {
                    let chunk = chunk_result.map_err(|e: crate::llm::provider::LlmError| {
                        StepError::ActionFailed {
                            reason: e.to_string(),
                        }
                    })?;

                    // Append this chunk's delta to the assembled response
                    full_response.push_str(&chunk.delta);

                    // Emit this chunk to the output sink if configured
                    if let Some(sink) = &self.output_sink {
                        let step_name = {
                            let ctx_lock = ctx.lock().await;
                            ctx_lock.step_name.clone()
                        };
                        sink.emit(OutputEvent::LlmChunk {
                            step: step_name,
                            delta: chunk.delta.clone(),
                        })
                        .await;
                    }
                }

                // Wire budget tracking (estimate based on assembled response length)
                {
                    let mut ctx_lock = ctx.lock().await;
                    ctx_lock.budget.llm_calls_used += 1;
                    // Rough token estimate: ~4 chars per token
                    let token_estimate = (full_response.len() as f64 / 4.0) as u32;
                    let cost = (token_estimate as f64) * 0.000001;
                    if let Some(remaining) = ctx_lock.budget.remaining_usd {
                        ctx_lock.budget.remaining_usd = Some((remaining - cost).max(0.0));
                    }

                    // Append resolved prompts to conversation history
                    ctx_lock
                        .conversation_history
                        .push(crate::llm::provider::ChatRole::User, resolved_user.clone());
                    ctx_lock.conversation_history.push(
                        crate::llm::provider::ChatRole::Assistant,
                        full_response.clone(),
                    );
                }

                Ok(StepOutput::new(full_response))
            }

            StepAction::ToolUseLoop {
                system,
                user,
                model,
                tools: _allowed_tools,
                max_rounds,
                stop_condition,
            } => {
                // ReAct tool-use loop with parallel multi-tool dispatch.
                //
                // Each round:
                //   1. Call LLM with tool schemas attached.
                //   2. If response contains tool_calls → dispatch ALL of them in parallel.
                //   3. Append each tool result as a `tool` role message.
                //   4. Loop back to step 1 with updated history.
                //   5. Stop when LLM returns text-only (no tool_calls), or max_rounds reached.
                let client = self
                    .llm_client
                    .as_ref()
                    .ok_or_else(|| StepError::ActionFailed {
                        reason: "no LLM client configured".into(),
                    })?;

                // Resolve template placeholders once at the start
                let (resolved_system, resolved_user_initial) = {
                    let ctx_lock = ctx.lock().await;
                    (
                        resolve_template(system, &ctx_lock),
                        resolve_template(user, &ctx_lock),
                    )
                };

                // Build tool schemas from the registry for the declared tool names.
                let tool_schemas: Vec<crate::llm::provider::ToolSchema> = _allowed_tools
                    .iter()
                    .filter_map(|name| self.tool_registry.get(name))
                    .map(|t| crate::llm::provider::ToolSchema {
                        name: t.name().to_string(),
                        description: t.description().to_string(),
                        parameters: t.schema(),
                    })
                    .collect();

                let mut history = crate::llm::provider::MessageHistory {
                    messages: vec![],
                    conversation_id: None,
                };
                let mut final_response = String::new();
                let max_iterations = *max_rounds;
                let mut llm_calls: u32 = 0;

                'outer: for round in 0..max_iterations {
                    let model_str = model.model.clone();
                    let user_message = if round == 0 {
                        resolved_user_initial.clone()
                    } else {
                        // Subsequent rounds: re-send original user message (history carries context)
                        resolved_user_initial.clone()
                    };

                    let req = crate::llm::LlmRequest {
                        system: resolved_system.clone(),
                        user: user_message.clone(),
                        model: model_str,
                        max_tokens: None,
                        history: if !history.messages.is_empty() {
                            Some(history.clone())
                        } else {
                            None
                        },
                        temperature: None,
                        tools: if !tool_schemas.is_empty() {
                            Some(tool_schemas.clone())
                        } else {
                            None
                        },
                    };

                    let response =
                        client
                            .complete(req)
                            .await
                            .map_err(|e| StepError::ActionFailed {
                                reason: e.to_string(),
                            })?;
                    llm_calls += 1;

                    // Append assistant turn to history
                    if !response.content.is_empty() {
                        history.messages.push(crate::llm::provider::ChatMessage {
                            role: crate::llm::provider::ChatRole::Assistant,
                            content: response.content.clone(),
                        });
                    }
                    final_response = response.content.clone();

                    // Check if LLM returned tool calls; if not, stop the loop
                    let has_tool_calls = response
                        .tool_calls
                        .as_ref()
                        .map(|v| !v.is_empty())
                        .unwrap_or(false);

                    if !has_tool_calls {
                        // No tool calls — text-only response, stop loop
                        break 'outer;
                    }

                    // Dispatch all tool calls in parallel
                    let tool_calls = response.tool_calls.unwrap();

                    // Dispatch all tool calls in parallel
                    use tokio::task::JoinSet;
                    let mut join_set: JoinSet<(String, String, Result<String, String>)> =
                        JoinSet::new();

                    for tc in tool_calls {
                        let tool_name = tc.name.clone();
                        let tool_args = tc.arguments.clone();
                        let call_id = format!("call_{}", tool_name);
                        let runner_clone = self.clone();
                        let ctx_clone = ctx.clone();

                        join_set.spawn(async move {
                            // Build a temp mutable ctx for the tool call
                            let mut temp_ctx = ctx_clone.lock().await.clone();
                            let result = runner_clone
                                .execute_tool_call(&tool_name, &tool_args, &mut temp_ctx)
                                .await;
                            let output = match result {
                                Ok(out) => out.raw,
                                Err(e) => format!("Error: {}", e),
                            };
                            (call_id, tool_name, Ok(output))
                        });
                    }

                    // Collect all tool results and add to history
                    while let Some(join_result) = join_set.join_next().await {
                        match join_result {
                            Ok((_call_id, tool_name, Ok(output))) => {
                                // Emit ToolChunk event to output sink
                                if let Some(sink) = &self.output_sink {
                                    let step_name = ctx.lock().await.step_name.clone();
                                    sink.emit(OutputEvent::ToolChunk {
                                        step: step_name,
                                        tool: tool_name.clone(),
                                        delta: output.clone(),
                                    })
                                    .await;
                                }
                                // Add tool result to history as a tool message
                                history.messages.push(crate::llm::provider::ChatMessage {
                                    role: crate::llm::provider::ChatRole::Tool,
                                    content: format!("[{}] {}", tool_name, output),
                                });
                            }
                            Ok((_call_id, tool_name, Err(e))) => {
                                history.messages.push(crate::llm::provider::ChatMessage {
                                    role: crate::llm::provider::ChatRole::Tool,
                                    content: format!("[{}] Error: {}", tool_name, e),
                                });
                            }
                            Err(join_err) => {
                                return Err(StepError::ActionFailed {
                                    reason: format!("tool task join error: {}", join_err),
                                });
                            }
                        }
                    }

                    // Check stop condition after tool dispatch
                    match stop_condition {
                        StopCondition::MaxRounds => {
                            if round + 1 >= max_iterations {
                                break 'outer;
                            }
                        }
                        StopCondition::Pattern(pattern) => {
                            if let Ok(regex) = regex::Regex::new(pattern) {
                                if regex.is_match(&final_response) {
                                    break 'outer;
                                }
                            }
                        }
                        StopCondition::TextOnly => {
                            // Continue looping — we had tool calls this round
                        }
                    }
                }

                // Wire budget tracking
                {
                    let mut ctx_lock = ctx.lock().await;
                    ctx_lock.budget.llm_calls_used =
                        ctx_lock.budget.llm_calls_used.saturating_add(llm_calls);
                    let token_estimate = (final_response.len() as f64 / 4.0) as u32;
                    let cost = (token_estimate as f64) * 0.000001;
                    if let Some(remaining) = ctx_lock.budget.remaining_usd {
                        ctx_lock.budget.remaining_usd = Some((remaining - cost).max(0.0));
                    }
                }

                Ok(StepOutput::new(final_response))
            }
        }
    }

    /// Execute a tool call with full 8-step protocol
    async fn execute_tool_call(
        &self,
        tool_name: &str,
        args: &Value,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        let audit_log = Arc::new(std::sync::Mutex::new(self.audit_log.clone()));

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

        // Step 6: Run tool with streaming
        let tool_context = ToolContext {
            filesystem_policy: ctx.filesystem_policy.clone(),
            network_policy: ctx.network_policy.clone(),
            allowed_tools: ctx.allowed_tools.clone(),
            audit_log: audit_log.clone(),
        };

        let tool_result = tool.call_streaming(args.clone(), tool_context).await;

        // Step 7: Handle result and record audit log
        match tool_result {
            Ok(chunks) => {
                // Assemble full output from chunks
                let mut full_output = String::new();

                for chunk in &chunks {
                    full_output.push_str(&chunk.delta);

                    // Emit this chunk to the output sink if not final and not empty
                    if !chunk.is_final && !chunk.delta.is_empty() {
                        if let Some(sink) = &self.output_sink {
                            sink.emit(OutputEvent::ToolChunk {
                                step: ctx.step_name.clone(),
                                tool: tool_name.to_string(),
                                delta: chunk.delta.clone(),
                            })
                            .await;
                        }
                    }
                }

                let output_bytes = full_output.len();

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

                // Wire budget tracking for tool calls
                ctx.budget.tool_calls_used += 1;

                Ok(StepOutput::new(full_output))
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

    /// Execute delegation to a named agent — Phase 4
    ///
    /// Follows the 8-step delegation protocol from architecture.md:
    /// 1. Check delegation policy
    /// 2. Check max delegation depth
    /// 3. Check allowed agent list
    /// 4. Create child context
    /// 5. Restrict child tools
    /// 6. Run child agent pipeline
    /// 7. Validate child output schema
    /// 8. Return child result to parent
    async fn execute_delegation(
        &mut self,
        agent_name: &str,
        delegate_input: &Value,
        expected_output_schema: Option<&Value>,
        delegation_policy: &crate::action::DelegationPolicy,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        let current_depth = ctx.delegation_depth;
        let parent_agent = ctx.agent_name.clone();

        // Step 1+2: Check max delegation depth
        if current_depth >= delegation_policy.max_depth {
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::DelegationFailed {
                    parent_agent: parent_agent.clone(),
                    child_agent: agent_name.to_string(),
                    depth: current_depth,
                    reason: format!(
                        "max delegation depth {} exceeded (current depth: {})",
                        delegation_policy.max_depth, current_depth
                    ),
                },
            });
            return Err(StepError::ActionFailed {
                reason: format!(
                    "delegation depth limit reached: max={}, current={}",
                    delegation_policy.max_depth, current_depth
                ),
            });
        }

        // Step 3: Check allowed agent list (empty list = allow all)
        if !delegation_policy.allowed_agents.is_empty()
            && !delegation_policy
                .allowed_agents
                .iter()
                .any(|a| a == agent_name)
        {
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: ctx.pipeline_name.clone(),
                step_name: ctx.step_name.clone(),
                event: AuditEvent::DelegationFailed {
                    parent_agent: parent_agent.clone(),
                    child_agent: agent_name.to_string(),
                    depth: current_depth,
                    reason: format!("agent '{}' not in allowed_agents list", agent_name),
                },
            });
            return Err(StepError::ActionFailed {
                reason: format!(
                    "agent '{}' not in allowed_agents: {:?}",
                    agent_name, delegation_policy.allowed_agents
                ),
            });
        }

        // Resolve the child agent from the registry
        let child_agent = match ctx.agent_registry.get(agent_name) {
            Some(a) => a,
            None => {
                let reason = format!("agent '{}' not found in registry", agent_name);
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: ctx.pipeline_name.clone(),
                    step_name: ctx.step_name.clone(),
                    event: AuditEvent::DelegationFailed {
                        parent_agent: parent_agent.clone(),
                        child_agent: agent_name.to_string(),
                        depth: current_depth,
                        reason: reason.clone(),
                    },
                });
                return Err(StepError::ActionFailed { reason });
            }
        };

        // Log delegation started
        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: ctx.pipeline_name.clone(),
            step_name: ctx.step_name.clone(),
            event: AuditEvent::DelegationStarted {
                parent_agent: parent_agent.clone(),
                child_agent: agent_name.to_string(),
                depth: current_depth,
            },
        });

        // Step 4+5: Create child runner with shared registries, restricted tools
        let child_tool_registry = if delegation_policy.inherit_tool_scope {
            self.tool_registry.clone()
        } else {
            Arc::new(ToolRegistry::new())
        };

        let mut child_runner =
            PipelineRunner::with_registries(child_tool_registry, ctx.agent_registry.clone());

        // Step 6: Run child agent pipeline
        // Build a temporary child agent with incremented delegation depth
        let mut child_policy = child_agent.policy.clone();
        // Inherit budget tracking if requested
        if delegation_policy.inherit_budget {
            child_policy.max_cost_usd = child_policy.max_cost_usd.or(ctx.budget.remaining_usd);
        }

        let child_agent_instance = crate::agent::Agent {
            name: child_agent.name.clone(),
            description: child_agent.description.clone(),
            pipeline: child_agent.pipeline.clone(),
            tools: child_agent.tools.clone(),
            skills: child_agent.skills.clone(),
            policy: child_policy,
        };

        let child_result = Box::pin(child_runner.run_with_delegation_depth(
            &child_agent_instance.pipeline.clone(),
            &child_agent_instance,
            delegate_input.clone(),
            current_depth + 1,
            Some(parent_agent.clone()),
        ))
        .await;

        match child_result {
            Ok(result) => {
                // Merge child trace entries into parent context
                ctx.trace
                    .entries
                    .extend(result.audit_log.entries().iter().map(|e| TraceEntry {
                        step_name: format!("{}.{}", agent_name, e.step_name),
                        status: format!("{:?}", e.event),
                        timestamp: e.timestamp,
                    }));

                // Merge child step results under namespaced keys
                for (k, v) in result.step_results {
                    ctx.step_results.insert(format!("{}.{}", agent_name, k), v);
                }

                // Step 7: Validate child output schema if required
                if delegation_policy.require_output_schema {
                    if let Some(schema) = expected_output_schema {
                        // Get the last step output from the child
                        if let Some(last_step) = result.steps_passed.last() {
                            let key = format!("{}.{}", agent_name, last_step);
                            if let Some(step_result) = ctx.step_results.get(&key) {
                                // Try to parse output as JSON for schema validation
                                if let Ok(output_value) =
                                    serde_json::from_str::<Value>(&step_result.output.raw)
                                {
                                    if let Ok(validator) = jsonschema::JSONSchema::compile(schema) {
                                        if let Err(errors) = validator.validate(&output_value) {
                                            let msgs: Vec<String> =
                                                errors.map(|e| e.to_string()).collect();
                                            return Err(StepError::ActionFailed {
                                                reason: format!(
                                                    "delegated agent '{}' output failed schema validation: {}",
                                                    agent_name,
                                                    msgs.join("; ")
                                                ),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Step 8: Log delegation completed, return result
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: ctx.pipeline_name.clone(),
                    step_name: ctx.step_name.clone(),
                    event: AuditEvent::DelegationCompleted {
                        parent_agent: parent_agent.clone(),
                        child_agent: agent_name.to_string(),
                        depth: current_depth,
                    },
                });

                // Build summary output from child result
                let summary = format!(
                    "Delegation to '{}' completed: {} steps passed, {} steps failed",
                    agent_name,
                    result.steps_passed.len(),
                    result.steps_failed.len()
                );
                Ok(StepOutput::new(summary))
            }
            Err(e) => {
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: ctx.pipeline_name.clone(),
                    step_name: ctx.step_name.clone(),
                    event: AuditEvent::DelegationFailed {
                        parent_agent: parent_agent.clone(),
                        child_agent: agent_name.to_string(),
                        depth: current_depth,
                        reason: e.to_string(),
                    },
                });
                Err(StepError::ActionFailed {
                    reason: format!("delegation to '{}' failed: {}", agent_name, e),
                })
            }
        }
    }

    /// Run a pipeline with an explicit delegation depth (used for child agents)
    pub async fn run_with_delegation_depth(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
        delegation_depth: u32,
        parent_agent: Option<String>,
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
        ctx.agent_registry = self.agent_registry.clone();
        ctx.tool_registry = self.tool_registry.clone();
        ctx.skill_registry = self.skill_registry.clone();
        ctx.llm_client = self.llm_client.clone();
        ctx.delegation_depth = delegation_depth;
        ctx.parent_agent = parent_agent;

        let mut steps_passed = Vec::new();
        let mut steps_failed = Vec::new();

        for step in &pipeline.steps {
            let mut retry_count = 0;
            let max_retries = pipeline.max_retries;
            let mut step_success = false;

            while retry_count <= max_retries && !step_success {
                ctx.step_name = step.name.clone();
                ctx.input = input.clone();
                ctx.allowed_tools = step.tools.clone();

                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: pipeline.name.clone(),
                    step_name: step.name.clone(),
                    event: AuditEvent::StepStarted,
                });

                // Run guard_in
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

                // Execute action — DelegateAgent handled separately for &mut self access
                let action_result_d = if let StepAction::DelegateAgent {
                    agent: ref agent_name,
                    ref input,
                    ref expected_output_schema,
                    ref delegation_policy,
                } = step.action
                {
                    self.execute_delegation(
                        agent_name,
                        input,
                        expected_output_schema.as_ref(),
                        delegation_policy,
                        &mut ctx,
                    )
                    .await
                } else {
                    let ctx_arc = Arc::new(TokioMutex::new(ctx.clone()));
                    let action_result = self.execute_action(&step.action, ctx_arc.clone()).await;
                    // Merge back any changes
                    ctx = match Arc::try_unwrap(ctx_arc) {
                        Ok(mutex) => mutex.into_inner(),
                        Err(arc) => {
                            let rt = tokio::runtime::Runtime::new().unwrap();
                            rt.block_on(async { arc.lock().await }).clone()
                        }
                    };
                    action_result
                };

                match action_result_d {
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
                                steps_failed.push(step.name.clone());
                                return Err(PipelineError::StepFailed {
                                    step: step.name.clone(),
                                    error: e,
                                });
                            }
                        }
                    }
                }

                ctx.trace.append(TraceEntry {
                    step_name: step.name.clone(),
                    status: "executed".to_string(),
                    timestamp: Utc::now(),
                });

                // Run guard_out
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

                // Run verdict
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

                        let result = StepResult {
                            step_name: step.name.clone(),
                            output: ctx
                                .output
                                .clone()
                                .unwrap_or_else(|| StepOutput::new("(no output)".to_string())),
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

                        // Emit step completion event to output sink
                        if let Some(sink) = &self.output_sink {
                            sink.emit(OutputEvent::StepCompleted {
                                step: step.name.clone(),
                                output: ctx
                                    .output
                                    .clone()
                                    .unwrap_or_else(|| StepOutput::new("(no output)".to_string())),
                            })
                            .await;
                        }
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

                                // Emit step completion event to output sink
                                if let Some(sink) = &self.output_sink {
                                    sink.emit(OutputEvent::StepCompleted {
                                        step: step.name.clone(),
                                        output: ctx.output.clone().unwrap_or_else(|| {
                                            StepOutput::new("(no output)".to_string())
                                        }),
                                    })
                                    .await;
                                }
                                break;
                            }
                            FailureMode::Fallback(_) => {
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

        let success = steps_failed.is_empty();
        let pipeline_result = PipelineResult {
            pipeline_name: pipeline.name.clone(),
            steps_passed,
            steps_failed,
            step_results: ctx.step_results,
            audit_log: self.audit_log.clone(),
            success,
        };

        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: pipeline.name.clone(),
            step_name: String::new(),
            event: if success {
                AuditEvent::PipelineCompleted {
                    steps_passed: pipeline_result.steps_passed.len() as u32,
                    steps_failed: pipeline_result.steps_failed.len() as u32,
                }
            } else {
                AuditEvent::PipelineFailed {
                    reason: format!("Failed steps: {:?}", pipeline_result.steps_failed),
                }
            },
        });

        // Emit pipeline completion event to output sink
        if let Some(sink) = &self.output_sink {
            sink.emit(OutputEvent::PipelineCompleted {
                result: pipeline_result.clone(),
            })
            .await;
        }

        Ok(pipeline_result)
    }

    /// Topologically sort pipeline steps based on dependencies (Phase 9 DAG support)
    pub fn topological_sort(pipeline: &Pipeline) -> Result<Vec<usize>, PipelineError> {
        let n = pipeline.steps.len();
        let mut sorted = Vec::new();
        let mut visited = vec![false; n];
        let mut visiting = vec![false; n];

        fn visit(
            node: usize,
            steps: &[crate::pipeline::AgentStep],
            visited: &mut [bool],
            visiting: &mut [bool],
            sorted: &mut Vec<usize>,
        ) -> Result<(), String> {
            if visited[node] {
                return Ok(());
            }
            if visiting[node] {
                return Err("Circular dependency detected".to_string());
            }

            visiting[node] = true;

            for dep in &steps[node].dependencies {
                // Find step index by name
                if let Some(dep_idx) = steps.iter().position(|s| &s.name == dep) {
                    visit(dep_idx, steps, visited, visiting, sorted)?;
                }
            }

            visiting[node] = false;
            visited[node] = true;
            sorted.push(node);
            Ok(())
        }

        for i in 0..n {
            if !visited[i] {
                visit(i, &pipeline.steps, &mut visited, &mut visiting, &mut sorted).map_err(
                    |reason| PipelineError::StepFailed {
                        step: format!("DAG validation"),
                        error: StepError::ActionFailed { reason },
                    },
                )?;
            }
        }

        Ok(sorted)
    }

    /// Execute pipeline with DAG support and parallel execution (Phase 9)
    pub async fn run_with_dag(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
    ) -> Result<PipelineResult, PipelineError> {
        // Validate DAG
        let _sorted_indices = Self::topological_sort(pipeline)?;

        // For now, fall back to regular execution
        // Full DAG + parallel execution would require additional logic
        self.run(pipeline, agent, input).await
    }

    /// Run a pipeline with hot-reload support (Phase 9)
    pub async fn run_hot(
        &mut self,
        hot_reload_handle: &crate::pipeline::HotReloadHandle,
        agent: &Agent,
        input: Value,
    ) -> Result<PipelineResult, PipelineError> {
        let pipeline = hot_reload_handle.get_pipeline().await;
        self.run(&pipeline, agent, input).await
    }

    /// Execute a pipeline step with plugin hooks (Phase 9)
    #[allow(dead_code)]
    async fn execute_step_with_plugins(
        &mut self,
        step: &crate::pipeline::AgentStep,
        ctx: &mut StepContext,
        plugins: &crate::pipeline::PluginRegistry,
    ) -> Result<StepOutput, StepError> {
        // Call on_step_start hooks
        for plugin in plugins.plugins() {
            if let Err(e) = plugin.on_step_start(ctx).await {
                return Err(StepError::ActionFailed {
                    reason: format!("plugin hook on_step_start failed: {}", e),
                });
            }
        }

        // Execute the step action
        let ctx_arc = Arc::new(TokioMutex::new(ctx.clone()));
        let result = self.execute_action(&step.action, ctx_arc.clone()).await;
        // Merge back any changes
        *ctx = match Arc::try_unwrap(ctx_arc) {
            Ok(mutex) => mutex.into_inner(),
            Err(arc) => {
                let rt = tokio::runtime::Runtime::new().unwrap();
                rt.block_on(async { arc.lock().await }).clone()
            }
        };

        // Call on_step_end hooks (always called, even on error)
        if let Ok(ref output) = result {
            for plugin in plugins.plugins() {
                if let Err(e) = plugin.on_step_end(ctx, output).await {
                    return Err(StepError::ActionFailed {
                        reason: format!("plugin hook on_step_end failed: {}", e),
                    });
                }
            }
        }

        result
    }
}

impl Default for PipelineRunner {
    fn default() -> Self {
        Self::new()
    }
}
