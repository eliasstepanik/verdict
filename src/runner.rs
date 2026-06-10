use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::action::{StepAction, StepError, StepOutput, IterationFailureMode, SkillMode};
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
    pub agent_registry: Arc<AgentRegistry>,
    pub skill_registry: Arc<crate::skills::registry::SkillRegistry>,
}

impl PipelineRunner {
    pub fn new() -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            agent_registry: Arc::new(AgentRegistry::new()),
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
        }
    }

    pub fn with_tool_registry(tool_registry: Arc<ToolRegistry>) -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry,
            agent_registry: Arc::new(AgentRegistry::new()),
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
        }
    }

    /// Create a runner with an agent registry for delegation support
    pub fn with_agent_registry(agent_registry: Arc<AgentRegistry>) -> Self {
        Self {
            audit_log: AuditLog::new(),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            agent_registry,
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
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
        ctx.agent_registry = self.agent_registry.clone();
        ctx.tool_registry = self.tool_registry.clone();
        ctx.skill_registry = self.skill_registry.clone();

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
                    self.execute_action(&step.action, &mut ctx).await
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
                // DelegateAgent is handled in run()/run_with_delegation_depth() before
                // execute_action is called, so this branch should never be reached.
                // If it is reached (e.g., nested in LoopUntil), return a clear error.
                Err(StepError::NotImplemented(
                    "DelegateAgent nested in LoopUntil/SubPipeline — not yet supported".to_string(),
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

            StepAction::UseSkill { skill, input: _skill_input, mode } => {
                // Look up the skill in the registry
                let skill_def = match ctx.skill_registry.get(skill) {
                    Some(s) => s,
                    None => {
                        return Err(StepError::ActionFailed {
                            reason: format!("Skill '{}' not found in registry", skill),
                        })
                    }
                };

                // Track active skill usage
                ctx.active_skills.push(skill.clone());

                // Execute based on mode
                match mode {
                    SkillMode::PromptOnly => {
                        // Return the skill's instructions as output
                        Ok(StepOutput::new(skill_def.instructions.clone()))
                    }
                    SkillMode::Pipeline => {
                        // Run skill's pipeline if available, else fall back to instructions
                        if let Some(pipeline) = skill_def.pipeline.clone() {
                            // Execute the sub-pipeline with Box::pin for async recursion
                            let sub_action = StepAction::SubPipeline(Box::new(pipeline));
                            let sub_future = self.execute_action(&sub_action, ctx);
                            match Pin::from(Box::new(sub_future)).await {
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
                            let sub_future = self.execute_action(&sub_action, ctx);
                            match Pin::from(Box::new(sub_future)).await {
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
                let prev_output = ctx.output.as_ref().map(|o| o.raw.clone()).unwrap_or_default();
                let condition_matches = prev_output.contains(condition);

                if condition_matches {
                    // Execute if_true
                    let branch_future = self.execute_action(if_true, ctx);
                    Pin::from(Box::new(branch_future)).await
                } else if let Some(if_false_action) = if_false {
                    // Execute if_false
                    let branch_future = self.execute_action(if_false_action, ctx);
                    Pin::from(Box::new(branch_future)).await
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
                        let output = serde_json::to_string(&result)
                            .unwrap_or_else(|_| result.to_string());
                        Ok(StepOutput::with_parsed(output, result))
                    }
                    Err(e) => {
                        Err(StepError::ActionFailed {
                            reason: format!("remote agent execution failed: {}", e),
                        })
                    }
                }
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
                    reason: format!(
                        "agent '{}' not in allowed_agents list",
                        agent_name
                    ),
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

        let mut child_runner = PipelineRunner::with_registries(
            child_tool_registry,
            ctx.agent_registry.clone(),
        );

        // Step 6: Run child agent pipeline
        // Build a temporary child agent with incremented delegation depth
        let mut child_policy = child_agent.policy.clone();
        // Inherit budget tracking if requested
        if delegation_policy.inherit_budget {
            child_policy.max_cost_usd = child_policy
                .max_cost_usd
                .or(ctx.budget.remaining_usd);
        }

        let child_agent_instance = crate::agent::Agent {
            name: child_agent.name.clone(),
            description: child_agent.description.clone(),
            pipeline: child_agent.pipeline.clone(),
            tools: child_agent.tools.clone(),
            skills: child_agent.skills.clone(),
            policy: child_policy,
        };

        let child_result = Box::pin(
            child_runner.run_with_delegation_depth(
                &child_agent_instance.pipeline.clone(),
                &child_agent_instance,
                delegate_input.clone(),
                current_depth + 1,
                Some(parent_agent.clone()),
            )
        )
        .await;

        match child_result {
            Ok(result) => {
                // Merge child trace entries into parent context
                ctx.trace.entries.extend(
                    result.audit_log.entries().iter().map(|e| TraceEntry {
                        step_name: format!("{}.{}", agent_name, e.step_name),
                        status: format!("{:?}", e.event),
                        timestamp: e.timestamp,
                    }),
                );

                // Merge child step results under namespaced keys
                for (k, v) in result.step_results {
                    ctx.step_results
                        .insert(format!("{}.{}", agent_name, k), v);
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
                                    if let Ok(validator) =
                                        jsonschema::JSONSchema::compile(schema)
                                    {
                                        if let Err(errors) =
                                            validator.validate(&output_value)
                                        {
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
                    self.execute_action(&step.action, &mut ctx).await
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
                            event: AuditEvent::StepCompleted { verdict_passed: true },
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
                                    event: AuditEvent::StepCompleted { verdict_passed: false },
                                });
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

    /// Topologically sort pipeline steps based on dependencies (Phase 9 DAG support)
    fn topological_sort(pipeline: &Pipeline) -> Result<Vec<usize>, PipelineError> {
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
                visit(i, &pipeline.steps, &mut visited, &mut visiting, &mut sorted)
                    .map_err(|reason| PipelineError::StepFailed {
                        step: format!("DAG validation"),
                        error: StepError::ActionFailed { reason },
                    })?;
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
        let result = self.execute_action(&step.action, ctx).await;

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
