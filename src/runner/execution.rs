use super::PipelineRunner;
use super::PipelineError;
use crate::action::{StepAction, StepError, StepOutput};
use crate::agent::Agent;
use crate::audit::{AuditEntry, AuditEvent};
use crate::context::{StepContext, StepResult};
use crate::guards::{GuardEngine, GuardPhase};
use crate::pipeline::Pipeline;
use crate::skills::skill::SkillSet;
use async_recursion::async_recursion;
use chrono::Utc;
use serde_json::Value;
use std::collections::VecDeque;
use tempfile;
use tracing::error;

// Free functions (resolve_template, strip_xml_tool_calls, parse_xml_tool_calls)
// are now in llm_synthesis.rs and re-exported via mod.rs



impl PipelineRunner {
    /// Topologically sort pipeline steps based on dependencies (Kahn's algorithm)
    /// Returns indices in execution order, or error if cycle is detected or dependency is missing.
    pub fn topological_sort(&self, pipeline: &Pipeline) -> Result<Vec<usize>, PipelineError> {
        let n = pipeline.steps.len();

        // Build a map from step name to index
        let name_to_idx: std::collections::HashMap<&str, usize> = pipeline
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name.as_str(), i))
            .collect();

        // Initialize in-degree and adjacency list
        let mut in_degree = vec![0usize; n];
        let mut adj: Vec<Vec<usize>> = vec![vec![]; n];

        // Build dependency graph
        for (i, step) in pipeline.steps.iter().enumerate() {
            for dep_name in &step.dependencies {
                match name_to_idx.get(dep_name.as_str()) {
                    Some(&dep_idx) => {
                        // Edge from dep_idx to i (i depends on dep_idx)
                        adj[dep_idx].push(i);
                        in_degree[i] += 1;
                    }
                    None => {
                        return Err(PipelineError::StepFailed {
                            step: step.name.clone(),
                            error: StepError::ActionFailed {
                                reason: format!("Unknown dependency: {}", dep_name),
                            },
                        });
                    }
                }
            }
        }

        // Kahn's algorithm
        let mut queue: VecDeque<usize> = (0..n).filter(|&i| in_degree[i] == 0).collect();
        let mut order = Vec::new();

        while let Some(idx) = queue.pop_front() {
            order.push(idx);
            for &next in &adj[idx] {
                in_degree[next] -= 1;
                if in_degree[next] == 0 {
                    queue.push_back(next);
                }
            }
        }

        // Check for cycles
        if order.len() != n {
            return Err(PipelineError::StepFailed {
                step: "DAG".into(),
                error: StepError::ActionFailed {
                    reason: "Cycle detected in step dependencies".into(),
                },
            });
        }

        Ok(order)
    }



    /// Execute a single step action — Phase 3 onwards
    ///
    /// # Known Issue
    /// The async_recursion macro has a limitation with the StepAction::Custom variant.
    /// The Custom closure type (`Arc<dyn Fn(&StepContext) -> ... + Send + Sync>`) contains
    /// a reference parameter that causes lifetime inference issues when the macro tries
    /// to box the future. This results in a compiler error:
    ///   "implementation of `FnOnce` is not general enough"
    ///
    /// This is a known limitation of the async_recursion macro, not a correctness issue
    /// with the code. A future refactor should consider:
    /// - Using a different recursion pattern (trampolining, manual Box::pin, etc.)
    /// - Changing the Custom closure type to avoid reference parameters
    /// - Using owned values instead of references in the closure signature
    ///
    /// For now, Custom actions are handled before async_recursion dispatch.
    pub async fn execute_action(
        &mut self,
        action: &StepAction,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        // Handle Custom here, outside async_recursion to avoid HRTB lifetime issues
        if let StepAction::Custom(f) = action {
            return f(ctx).map_err(|e| e);
        }
        self.execute_action_inner(action, ctx).await
    }

    #[async_recursion(?Send)]
    async fn execute_action_inner(
        &mut self,
        action: &StepAction,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        match action {
            // ========== LlmCall ==========
            StepAction::LlmCall {
                system,
                user,
                model,
                conversation_id,
                append_to_history,
            } => {
                self.handle_llm_call(system, user, model, conversation_id, append_to_history, ctx)
                    .await
            }

            // ========== LlmCallStreaming ==========
            StepAction::LlmCallStreaming {
                system,
                user,
                model,
                conversation_id,
                append_to_history,
            } => {
                self.handle_llm_call_streaming(system, user, model, conversation_id, append_to_history, ctx)
                    .await
            }

            // ========== ToolCall ==========
            StepAction::ToolCall { tool, args } => {
                self.handle_tool_call(ctx, tool, args).await
            }

            // ========== SubPipeline ==========
            StepAction::SubPipeline(pipeline) => {
                self.handle_subpipeline(pipeline, ctx).await
            }

            // ========== LoopUntil ==========
            StepAction::LoopUntil {
                body,
                condition,
                max_iterations,
                on_iteration_failure,
            } => {
                self.handle_loop_until(body, condition, max_iterations, on_iteration_failure, ctx)
                    .await
            }

            // ========== UseSkill ==========
            StepAction::UseSkill {
                skill,
                input: _input,
                mode,
            } => {
                self.handle_use_skill(skill, mode, ctx).await
            }

            // ========== Branch ==========
            StepAction::Branch {
                condition,
                if_true,
                if_false,
            } => {
                self.handle_branch(condition, if_true, if_false, ctx).await
            }

            // ========== RemoteAgent ==========
            StepAction::RemoteAgent {
                endpoint,
                agent_name,
                payload,
            } => {
                self.handle_remote_agent(endpoint, agent_name, payload, ctx).await
            }

            // ========== ToolUseLoop ==========
            StepAction::ToolUseLoop {
                system,
                user,
                model,
                tools,
                max_rounds,
                stop_condition,
            } => {
                self.handle_tool_use_loop(system, user, model, tools, max_rounds, stop_condition, ctx)
                    .await
            }

            // ========== UserInput ==========
            StepAction::UserInput { prompt, schema: _ } => {
                self.handle_user_input(prompt, ctx).await
            }

            // ========== DelegateAgent ==========
            // NOTE: DelegateAgent should be handled in the main run() method before execute_action.
            // If we reach here, it means the caller misused the runner.
            StepAction::DelegateAgent { .. } => Err(StepError::ActionFailed {
                reason:
                    "DelegateAgent should be handled by PipelineRunner::run(), not execute_action()"
                        .into(),
            }),

            // ========== Sleep ==========
            StepAction::Sleep { duration_ms } => {
                self.handle_sleep(duration_ms, ctx).await
            }

            // ========== SleepUntil ==========
            StepAction::SleepUntil { timestamp } => {
                self.handle_sleep_until(timestamp, ctx).await
            }

            // ========== ForEach ==========
            StepAction::ForEach {
                input_array_key,
                body: _,
                concurrency,
                collect_results,
            } => {
                self.handle_for_each(input_array_key, concurrency, collect_results, ctx)
                    .await
            }

            // ========== Suspend ==========
            StepAction::Suspend {
                reason,
                resume_schema,
                timeout_seconds: _timeout,
            } => {
                self.handle_suspend(reason, resume_schema, ctx).await
            }

            // All other variants (Custom cannot be handled here due to async_recursion
            // macro limitations with closure lifetime constraints - known compiler limitation)
            _ => Err(StepError::ActionFailed {
                reason: "Unhandled step action variant (including Custom)".into(),
            }),
        }
    }

    /// Execute a pipeline with a specified delegation depth and parent agent
    /// This is the internal entry point used for recursive delegation.
    #[async_recursion(?Send)]
    pub async fn run_with_delegation_depth(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
        delegation_depth: u32,
        parent_agent: String,
    ) -> Result<super::PipelineResult, PipelineError> {
        self.run_with_delegation_depth_and_budget(
            pipeline,
            agent,
            input,
            delegation_depth,
            parent_agent,
            None,
        )
        .await
    }

    pub async fn run_with_delegation_depth_and_budget(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
        delegation_depth: u32,
        parent_agent: String,
        inherited_budget: Option<crate::context::BudgetState>,
    ) -> Result<super::PipelineResult, PipelineError> {
        // Run the pipeline with delegation context and optional inherited budget injected
        self.run_internal(
            pipeline,
            agent,
            input,
            Some((delegation_depth, parent_agent)),
            inherited_budget,
        )
        .await
    }

    /// Execute a pipeline with an agent
    #[async_recursion(?Send)]
    pub async fn run(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
    ) -> Result<super::PipelineResult, PipelineError> {
        self.run_internal(pipeline, agent, input, None, None).await
    }

    /// Internal pipeline runner with optional delegation context and inherited budget
    #[async_recursion(?Send)]
    async fn run_internal(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
        delegation_context: Option<(u32, String)>, // (delegation_depth, parent_agent)
        inherited_budget: Option<crate::context::BudgetState>,
    ) -> Result<super::PipelineResult, PipelineError> {
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
        ctx.agent_policy = agent.policy.clone();
        ctx.agent_registry = self.agent_registry.clone();
        ctx.tool_registry = self.tool_registry.clone();
        ctx.skill_registry = self.skill_registry.clone();
        ctx.llm_client = self.llm_client.clone();
        ctx.memory = self.memory.clone();

        // Apply delegation context if provided
        if let Some((depth, parent)) = delegation_context {
            ctx.delegation_depth = depth;
            ctx.parent_agent = Some(parent);
        }

        // Apply inherited budget if provided (for inherit_budget delegation policy)
        if let Some(budget) = inherited_budget {
            ctx.budget = budget;
        }

        // Set up workspace isolation (TempDir guard held for entire run)
        let _temp_workspace = match &agent.policy.filesystem_policy.workspace_isolation {
            crate::agent::WorkspaceIsolation::None => None,
            crate::agent::WorkspaceIsolation::TempDir => {
                let temp_dir = tempfile::TempDir::new()
                    .map_err(|e| PipelineError::RuntimeSetupFailed(
                        format!("Failed to create temp workspace: {}", e)
                    ))?;
                let temp_path = temp_dir.path().to_path_buf();
                ctx.filesystem_policy.workspace_root = temp_path;
                Some(temp_dir)
            }
            crate::agent::WorkspaceIsolation::Sandboxed(path) => {
                if !path.exists() {
                    return Err(PipelineError::RuntimeSetupFailed(
                        format!("Sandboxed workspace path does not exist: {}", path.display())
                    ));
                }
                if !path.is_dir() {
                    return Err(PipelineError::RuntimeSetupFailed(
                        format!("Sandboxed workspace path is not a directory: {}", path.display())
                    ));
                }
                ctx.filesystem_policy.workspace_root = path.clone();
                None
            }
        };

        let mut steps_passed = Vec::new();
        #[allow(unused_mut)]
        let mut steps_failed = Vec::new();

        // Compute topological order based on dependencies
        let execution_order = self.topological_sort(pipeline)?;

        // Process steps, batching consecutive parallel steps
        let mut i = 0;
        while i < execution_order.len() {
            let step_idx = execution_order[i];
            let step = &pipeline.steps[step_idx];

            if step.parallel {
                // Collect all consecutive parallel steps
                let mut batch = vec![step_idx];
                let mut j = i + 1;
                while j < execution_order.len() {
                    let next_idx = execution_order[j];
                    if pipeline.steps[next_idx].parallel {
                        batch.push(next_idx);
                        j += 1;
                    } else {
                        break;
                    }
                }
                i = j;

                // Execute batch via the parallel batch executor. It reuses the
                // step_exec phases, so all StepAction variants are supported and
                // guard_in is evaluated for parallel steps.
                match super::parallel::execute_parallel_batch(
                    self,
                    pipeline,
                    &mut ctx,
                    &batch,
                    &agent.policy.allowed_tools,
                )
                .await
                {
                    Ok(batch_results) => {
                        for (step_name, _) in batch_results {
                            steps_passed.push(step_name);
                        }
                    }
                    Err(e) => {
                        return Err(e);
                    }
                }
                continue;
            } else {
                i += 1;
            }

            let step = &pipeline.steps[step_idx].clone();

            ctx.step_name = step.name.clone();
            ctx.input = input.clone();

            // Compute effective tool scope for this step.
            // Shared with the parallel path via `step_exec::step_tool_scope`.
            ctx.allowed_tools =
                super::step_exec::step_tool_scope(&agent.policy.allowed_tools, step);

            // ===== Record StepStarted audit event =====
            // Shared with the parallel path via step_exec, so both emit an
            // identical event. Must be the first event recorded for this step
            // (integration_observability::test_audit_log_event_ordering_within_step).
            super::step_exec::emit_step_started(self, step, &ctx);

            // Handle guard_in failure based on FailureMode (before trying action)
            match GuardEngine::evaluate(&step.guard_in, &ctx).await {
                Ok(()) => {
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardPassed {
                            guard: step.guard_in.name(),
                        },
                    });
                }
                Err(e) => {
                    let guard_err: crate::guards::GuardError = e;
                    let err_str = format!("{guard_err}");
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardFailed {
                            guard: step.guard_in.name(),
                            reason: err_str.clone(),
                        },
                    });

                    match &pipeline.on_failure {
                        crate::pipeline::FailureMode::Skip => {
                            steps_failed.push(step.name.clone());
                            let sr = StepResult {
                                step_name: step.name.clone(),
                                output: StepOutput::new(String::new()),
                                verdict_passed: false,
                                error: Some(format!("guard_in failed: {err_str}")),
                            };
                            ctx.step_results.insert(step.name.clone(), sr);
                            self.audit_log.append(AuditEntry {
                                timestamp: Utc::now(),
                                pipeline_name: pipeline.name.clone(),
                                step_name: step.name.clone(),
                                event: AuditEvent::StepFailed {
                                    error: format!("guard_in failed: {err_str}"),
                                },
                            });
                            continue;
                        }
                        _ => {
                            return Err(PipelineError::GuardFailed {
                                step: step.name.clone(),
                                phase: GuardPhase::In,
                                error: guard_err,
                            });
                        }
                    }
                }
            }

            // Handle action with retry loop
            #[allow(unused_assignments)]
            let mut action_error: Option<StepError> = None;
            let mut retries_left = match &pipeline.on_failure {
                crate::pipeline::FailureMode::Retry => pipeline.max_retries,
                _ => 0,
            };

            loop {
                let action_result = if let StepAction::DelegateAgent {
                    agent: delegate_agent_name,
                    input: delegate_input,
                    expected_output_schema,
                    delegation_policy,
                    detached: _,
                } = &step.action
                {
                    self.execute_delegation(
                        delegate_agent_name,
                        delegate_input,
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
                        action_error = None;
                        break;
                    }
                    Err(e) => {
                        action_error = Some(e);
                        if retries_left > 0
                            && matches!(&pipeline.on_failure, crate::pipeline::FailureMode::Retry)
                        {
                            retries_left -= 1;
                        } else {
                            break;
                        }
                    }
                }
            }

            // Handle action failure based on FailureMode
            if action_error.is_some() {
                match &pipeline.on_failure {
                    crate::pipeline::FailureMode::Skip => {
                        steps_failed.push(step.name.clone());
                        let sr = StepResult {
                            step_name: step.name.clone(),
                            output: ctx
                                .output
                                .clone()
                                .unwrap_or_else(|| StepOutput::new(String::new())),
                            verdict_passed: false,
                            error: action_error.as_ref().map(|e| format!("{:?}", e)),
                        };
                        ctx.step_results.insert(step.name.clone(), sr);
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::StepFailed {
                                error: format!("{:?}", action_error),
                            },
                        });
                        continue;
                    }
                    crate::pipeline::FailureMode::Retry => {
                        return Err(PipelineError::MaxRetriesExceeded {
                            step: step.name.clone(),
                        });
                    }
                    crate::pipeline::FailureMode::Abort => {
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::StepFailed {
                                error: format!("{:?}", action_error),
                            },
                        });
                        return Err(PipelineError::StepFailed {
                            step: step.name.clone(),
                            error: action_error.unwrap(),
                        });
                    }
                    crate::pipeline::FailureMode::Fallback(fallback_pipeline) => {
                        let original_error = action_error.take().unwrap();
                        let step_name_clone = step.name.clone();

                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::FallbackTriggered {
                                step: step.name.clone(),
                                reason: format!("{:?}", original_error),
                            },
                        });

                        // A fallback pipeline is a substitute execution for the step that
                        // just failed, so it MUST run under the same security context.
                        //
                        // This handler previously built a fresh `PipelineRunner` and called
                        // the plain `run()` entry point with the unmodified `agent`, which
                        // silently reset:
                        //   * `delegation_depth` -> 0, so a delegated child sitting at
                        //     `max_delegation_depth` could launder unlimited further
                        //     delegation through its fallback pipeline
                        //   * `budget`           -> fresh, restarting cost/call accounting
                        //   * `filesystem_policy` -> re-resolved from the agent policy, so a
                        //     `WorkspaceIsolation::TempDir` agent minted a SECOND temp dir
                        //     instead of staying pinned to the parent's active sandbox
                        //
                        // Depth semantics: unlike `StepAction::SubPipeline` (which is a real
                        // nesting boundary and therefore uses `delegation_depth + 1`), a
                        // fallback replaces the failed step *in place* — it is the same
                        // logical step retried a different way, not a level deeper. So it
                        // inherits `ctx.delegation_depth` UNCHANGED. Containment still holds:
                        // any delegation or SubPipeline *inside* the fallback increments from
                        // that inherited depth, so a fallback at the cap cannot descend.
                        let fallback_depth = ctx.delegation_depth;

                        // Derive the fallback agent's policy from the PARENT's actual policy
                        // so cost/step/runtime caps, allowed agents, skills and tool scope all
                        // carry over. Tool scope stays at the agent level (not the failed
                        // step's narrowed scope) because each fallback step intersects the
                        // agent policy with its own declared `tools` during `run_internal`.
                        let mut fallback_policy = ctx.agent_policy.clone();

                        // Filesystem: inherit the parent's *resolved* policy.
                        // `ctx.filesystem_policy.workspace_root` already points at the active
                        // TempDir/Sandboxed root, so isolation is set to `None` to keep the
                        // fallback inside that same sandbox rather than creating a new one.
                        fallback_policy.filesystem_policy = ctx.filesystem_policy.clone();
                        fallback_policy.filesystem_policy.workspace_isolation =
                            crate::agent::WorkspaceIsolation::None;

                        // Network: inherit the parent's actual policy, not a fresh default.
                        fallback_policy.network_policy = ctx.network_policy.clone();

                        let fallback_agent = crate::agent::Agent {
                            name: agent.name.clone(),
                            description: agent.description.clone(),
                            pipeline: fallback_pipeline.as_ref().clone(),
                            tools: fallback_policy.allowed_tools.clone(),
                            skills: SkillSet {
                                skills: ctx.active_skills.clone(),
                            },
                            policy: fallback_policy,
                            scorers: agent.scorers.clone(),
                        };

                        let mut fallback_runner = PipelineRunner {
                            audit_log: crate::audit::AuditLog::new(),
                            tool_registry: self.tool_registry.clone(),
                            agent_registry: self.agent_registry.clone(),
                            skill_registry: self.skill_registry.clone(),
                            llm_client: self.llm_client.clone(),
                            output_sink: self.output_sink.clone(),
                            conversation_registry: self.conversation_registry.clone(),
                            context_store: self.context_store.clone(),
                            plugin_registry: self.plugin_registry.clone(),
                            auto_title_llm: self.auto_title_llm.clone(),
                            memory: self.memory.clone(),
                        };

                        // Run through the depth/budget-aware entry point — the same one
                        // `execute_delegation` and the SubPipeline handler use. The parent's
                        // spent budget goes in; the fallback's final budget comes back out on
                        // the returned `PipelineResult`, keeping accounting continuous.
                        let fallback_result = fallback_runner
                            .run_with_delegation_depth_and_budget(
                                fallback_pipeline,
                                &fallback_agent,
                                input.clone(),
                                fallback_depth,
                                ctx.parent_agent.clone().unwrap_or_else(|| ctx.agent_name.clone()),
                                Some(ctx.budget.clone()),
                            )
                            .await;

                        return match fallback_result {
                            Ok(result) => {
                                // Budget continuity: the fallback's spend is the run's spend.
                                ctx.budget = result.budget.clone();
                                Ok(result)
                            }
                            Err(_) => Err(PipelineError::StepFailed {
                                step: step_name_clone,
                                error: original_error,
                            }),
                        };
                    }
                }
            }

            // ===== Post-action phase (injection check, guard_out, output_schema,
            // verdict, StepCompleted) =====
            // Shared with the parallel path via step_exec::run_post_action so both
            // paths enforce injection protection and output_schema identically.
            // Previously duplicated inline here, which silently skipped both checks
            // for the default (parallel: false) path.
            let post_output = ctx
                .output
                .clone()
                .unwrap_or_else(|| StepOutput::new(String::new()));
            let post_output = match super::step_exec::run_post_action(self, step, &mut ctx, post_output).await
            {
                Ok(output) => output,
                Err(super::step_exec::PostActionError::GuardOut(guard_err)) => {
                    return Err(PipelineError::GuardFailed {
                        step: step.name.clone(),
                        phase: GuardPhase::Out,
                        error: guard_err,
                    });
                }
                Err(super::step_exec::PostActionError::Verdict(verdict_err)) => {
                    return Err(PipelineError::VerdictFailed {
                        step: step.name.clone(),
                        error: verdict_err,
                    });
                }
                Err(super::step_exec::PostActionError::Step(step_err)) => {
                    return Err(PipelineError::StepFailed {
                        step: step.name.clone(),
                        error: step_err,
                    });
                }
            };

            let sr = StepResult {
                step_name: step.name.clone(),
                output: post_output,
                verdict_passed: true,
                error: None,
            };
            ctx.step_results.insert(step.name.clone(), sr);
            steps_passed.push(step.name.clone());

            // Auto-save after each step
            if let Some(store) = &self.context_store {
                if let Err(e) = store.save(&ctx).await {
                    error!(error = %e, "ContextStore::save failed");
                }
            }

            // Check for cancellation before proceeding to next step (Phase 14)
            if ctx.cancellation_token.is_cancelled() {
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: pipeline.name.clone(),
                    step_name: step.name.clone(),
                    event: AuditEvent::PipelineFailed {
                        reason: "Cancelled by cancellation token".into(),
                    },
                });
                return Err(PipelineError::StepFailed {
                    step: step.name.clone(),
                    error: StepError::ActionFailed {
                        reason: "Cancelled".into(),
                    },
                });
            }
        }

        // Final audit log
        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: pipeline.name.clone(),
            step_name: String::new(),
            event: AuditEvent::PipelineCompleted {
                steps_passed: steps_passed.len() as u32,
                steps_failed: steps_failed.len() as u32,
            },
        });

        Ok(super::PipelineResult {
            pipeline_name: pipeline.name.clone(),
            steps_passed,
            steps_failed: steps_failed.clone(),
            step_results: ctx.step_results,
            audit_log: self.audit_log.clone(),
            success: steps_failed.is_empty(),
            total_cost_usd: ctx.budget.spent_usd,
            total_tokens_used: 0, // Will be updated when LLM calls are tracked
            log: vec![],          // Will be populated when logging is wired
            suspended: None,
             budget: ctx.budget.clone(),
        })
    }

    // build_skill_system_prompt and run_skill_eval are now in step_executor.rs
    // and re-exported via mod.rs
}


