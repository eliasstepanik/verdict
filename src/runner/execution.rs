use super::PipelineRunner;
use super::PipelineError;
use crate::action::{StepAction, StepError, StepOutput};
use crate::agent::{Agent, RemoteAgentClient};
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
                // A sub-pipeline is a delegation boundary and MUST inherit the parent's
                // security context exactly like `delegation::execute_delegation` does.
                //
                // This handler previously built an `AgentPolicy::default()` and called
                // the plain `run()` entry point, which silently reset:
                //   * `filesystem_policy`  -> fresh `current_dir()` root, dropping
                //     `WorkspaceIsolation::TempDir`/`Sandboxed` (writes escaped to the real repo)
                //   * `network_policy`     -> `DenyAll` (fail-closed, but still divergent)
                //   * `delegation_depth`   -> 0, letting a SubPipeline wrapper bypass
                //     `max_delegation_depth` / `Guard::MaxDelegationDepth` entirely
                //   * `budget`             -> fresh, resetting cost/call accounting
                //
                // Everything below derives from the parent context instead.
                let child_depth = ctx.delegation_depth + 1;

                // Enforce the parent's delegation depth cap on this boundary, mirroring
                // the depth check `execute_delegation` performs before recursing.
                if child_depth > ctx.agent_policy.max_delegation_depth {
                    return Err(StepError::ActionFailed {
                        reason: format!(
                            "SubPipeline delegation depth {} exceeds max {}",
                            child_depth, ctx.agent_policy.max_delegation_depth
                        ),
                    });
                }

                // Create a child runner with shared registries
                let mut child_runner = PipelineRunner {
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

                // Derive the sub-agent's policy from the PARENT's actual policy so that
                // cost/step/runtime caps, allowed agents and skills all carry over.
                let mut policy = ctx.agent_policy.clone();

                // Tool scope: the already-narrowed effective scope from the parent context
                // (agent ∩ pipeline ∩ step ∩ skill), so inner steps intersect against the
                // narrowed set rather than the agent-level default.
                policy.allowed_tools = ctx.allowed_tools.clone();

                // Filesystem: inherit the parent's *resolved* policy. `ctx.filesystem_policy`
                // already has `workspace_root` rewritten to the active TempDir/Sandboxed root
                // by `run_internal`, so isolation is set to `None` here to keep the child
                // pinned to that same sandbox instead of minting a second, unrelated temp dir
                // (which would hide the parent's files from the sub-pipeline).
                policy.filesystem_policy = ctx.filesystem_policy.clone();
                policy.filesystem_policy.workspace_isolation =
                    crate::agent::WorkspaceIsolation::None;

                // Network: inherit the parent's actual policy, not a fresh DenyAll.
                policy.network_policy = ctx.network_policy.clone();

                let agent = crate::agent::Agent {
                    name: ctx.agent_name.clone(),
                    description: "Child pipeline agent".into(),
                    pipeline: pipeline.as_ref().clone(),
                    tools: ctx.allowed_tools.clone(),
                    skills: SkillSet {
                        skills: ctx.active_skills.clone(),
                    },
                    policy,
                    scorers: Vec::new(),
                };

                // Run the child pipeline through the depth/budget-aware entry point —
                // the same one `execute_delegation` uses.
                let result = child_runner
                    .run_with_delegation_depth_and_budget(
                        pipeline,
                        &agent,
                        ctx.input.clone(),
                        child_depth,
                        ctx.agent_name.clone(),
                        Some(ctx.budget.clone()),
                    )
                    .await
                    .map_err(|e| StepError::ActionFailed {
                        reason: format!("SubPipeline failed: {}", e),
                    })?;

                // Propagate the child's budget back so cost/call accounting is continuous.
                ctx.budget = result.budget.clone();

                // Merge the child's audit entries into the parent log so the sub-pipeline
                // is not invisible to auditing.
                for entry in result.audit_log.entries() {
                    self.audit_log.append(entry.clone());
                }

                // Get the LAST step's output deterministically by finding the last step in
                // the pipeline and looking it up by name (not by HashMap iteration order).
                let output = pipeline
                    .steps
                    .last()
                    .and_then(|last_step| result.step_results.get(&last_step.name))
                    .map(|sr| sr.output.clone())
                    .unwrap_or_else(|| StepOutput::new(String::new()));

                Ok(output)
            }

            // ========== LoopUntil ==========
            StepAction::LoopUntil {
                body,
                condition,
                max_iterations,
                on_iteration_failure,
            } => {
                for iteration in 0u32..*max_iterations {
                    // Execute the body
                    let body_result = self.execute_action(body, ctx).await;

                    match body_result {
                        Ok(output) => {
                            ctx.output = Some(output);
                        }
                        Err(e) => {
                            match on_iteration_failure {
                                crate::action::IterationFailureMode::Retry => {
                                    // Continue to next iteration
                                    continue;
                                }
                                crate::action::IterationFailureMode::Skip => {
                                    // Skip to next iteration without error
                                    continue;
                                }
                                crate::action::IterationFailureMode::Abort => {
                                    // Fail the entire loop
                                    return Err(e);
                                }
                            }
                        }
                    }

                    // Check the condition guard
                    match GuardEngine::evaluate(condition, ctx).await {
                        Ok(()) => {
                            // Condition passed, exit the loop
                            break;
                        }
                        Err(_) => {
                            // Condition failed, continue looping
                            if iteration + 1 >= *max_iterations {
                                break; // Max iterations reached — exit loop normally
                            }
                        }
                    }
                }

                Ok(ctx
                    .output
                    .clone()
                    .unwrap_or_else(|| StepOutput::new(String::new())))
            }

            // ========== UseSkill ==========
            StepAction::UseSkill {
                skill,
                input: _input,
                mode,
            } => {
                let skill_def =
                    self.skill_registry
                        .get(skill)
                        .ok_or_else(|| StepError::ActionFailed {
                            reason: format!("Skill '{}' not found", skill),
                        })?;

                // Check required guards
                for guard in &skill_def.required_guards {
                    GuardEngine::evaluate(guard, ctx).await.map_err(
                        |e: crate::guards::GuardError| StepError::ActionFailed {
                            reason: format!("Skill required guard failed: {e}"),
                        },
                    )?;
                }

                // FIX #1: Narrow allowed_tools by skill's allowed_tools
                // Effective scope: agent ∩ pipeline ∩ step ∩ skill
                let saved_tools = ctx.allowed_tools.clone();
                ctx.allowed_tools = crate::toolset::ToolSet::Intersection(
                    Box::new(ctx.allowed_tools.clone()),
                    Box::new(skill_def.allowed_tools.clone()),
                );

                let result = match mode {
                    crate::action::SkillMode::PromptOnly => {
                        // If no LLM client, return the instructions directly
                        if self.llm_client.is_none() {
                            Ok(StepOutput::new(skill_def.instructions.clone()))
                        } else {
                            // Inject skill instructions and few-shot examples into the system prompt
                            let system_prompt = self.build_skill_system_prompt(
                                &skill_def.instructions,
                                &skill_def.examples,
                            );

                            let llm_call = StepAction::LlmCall {
                                system: system_prompt,
                                user: String::new(),
                                model: None,
                                conversation_id: None,
                                append_to_history: false,
                            };

                            self.execute_action(&llm_call, ctx).await
                        }
                    }

                    crate::action::SkillMode::Pipeline | crate::action::SkillMode::Auto => {
                        if let Some(pipeline) = &skill_def.pipeline {
                            // Run as a sub-pipeline
                            let sub_action = StepAction::SubPipeline(Box::new(pipeline.clone()));
                            self.execute_action(&sub_action, ctx).await
                        } else {
                            // Fall back to PromptOnly if no pipeline available (for both Pipeline and Auto modes)
                            // If no LLM client, return the instructions directly (same as PromptOnly without LLM)
                            if self.llm_client.is_none() {
                                Ok(StepOutput::new(skill_def.instructions.clone()))
                            } else {
                                // Inject skill instructions and few-shot examples
                                let system_prompt = self.build_skill_system_prompt(
                                    &skill_def.instructions,
                                    &skill_def.examples,
                                );

                                let llm_call = StepAction::LlmCall {
                                    system: system_prompt,
                                    user: String::new(),
                                    model: None,
                                    conversation_id: None,
                                    append_to_history: false,
                                };
                                self.execute_action(&llm_call, ctx).await
                            }
                        }
                    }
                };

                // Restore original tool scope after skill execution
                ctx.allowed_tools = saved_tools;

                // FEATURE: Run skill evaluation if present and step succeeded
                let mut final_result = result?;
                if let Some(skill_eval) = &skill_def.eval {
                    let eval_output =
                        self.run_skill_eval(skill_eval, &final_result, &skill_def.name)
                            .await;
                    // Attach eval result to output metadata (informational, non-blocking)
                    final_result.set_eval_result(eval_output);
                }

                Ok(final_result)
            }

            // ========== Branch ==========
            StepAction::Branch {
                condition,
                if_true,
                if_false,
            } => {
                let output_str = ctx
                    .output
                    .as_ref()
                    .map(|o| o.raw.clone())
                    .unwrap_or_default();

                if output_str.contains(condition) {
                    self.execute_action(if_true, ctx).await
                } else if let Some(false_action) = if_false {
                    self.execute_action(false_action, ctx).await
                } else {
                    // No false branch, return empty output
                    Ok(StepOutput::new(String::new()))
                }
            }

            // ========== RemoteAgent ==========
            StepAction::RemoteAgent {
                endpoint,
                agent_name,
                payload,
            } => {
                let client = RemoteAgentClient::new();
                let result = client
                    .execute(endpoint.as_str(), agent_name.as_str(), payload.clone())
                    .await
                    .map_err(|e| StepError::RemoteAgentFailed(e))?;

                Ok(StepOutput::new(result.to_string()))
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
                eprintln!("{} [y/N]: ", prompt);
                let stdin = std::io::stdin();
                let mut line = String::new();
                stdin
                    .read_line(&mut line)
                    .map_err(|e| StepError::ActionFailed {
                        reason: format!("Failed to read user input: {}", e),
                    })?;

                Ok(StepOutput::new(line.trim().to_string()))
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
                tokio::time::sleep(std::time::Duration::from_millis(*duration_ms)).await;
                Ok(StepOutput::new(format!("Slept for {}ms", duration_ms)))
            }

            // ========== SleepUntil ==========
            StepAction::SleepUntil { timestamp } => {
                let now = chrono::Utc::now();
                if *timestamp > now {
                    if let Ok(dur) = (*timestamp - now).to_std() {
                        tokio::time::sleep(dur).await;
                    }
                }
                Ok(StepOutput::new(format!("Slept until {}", timestamp)))
            }

            // ========== ForEach ==========
            StepAction::ForEach {
                input_array_key,
                body: _,
                concurrency,
                collect_results,
            } => {
                // Get items array from a prior step's output
                let items: Vec<serde_json::Value> = ctx
                    .step_results
                    .get(input_array_key.as_str())
                    .and_then(|r| r.output.parsed.as_ref())
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut results: Vec<StepOutput> = Vec::new();

                if *concurrency <= 1 {
                    // Sequential execution
                    for item in &items {
                        let output = StepOutput::with_parsed(item.to_string(), item.clone());
                        results.push(output);
                    }
                } else {
                    // Bounded parallel execution
                    use futures::stream::{self, StreamExt};
                    let outputs: Vec<StepOutput> = stream::iter(items.iter())
                        .map(|item| async move {
                            StepOutput::with_parsed(item.to_string(), item.clone())
                        })
                        .buffer_unordered(*concurrency)
                        .collect()
                        .await;
                    results = outputs;
                }

                if *collect_results {
                    let arr: Vec<serde_json::Value> = results
                        .iter()
                        .map(|r| {
                            r.parsed
                                .clone()
                                .unwrap_or(serde_json::Value::String(r.raw.clone()))
                        })
                        .collect();
                    let json = serde_json::Value::Array(arr);
                    Ok(StepOutput::with_parsed(json.to_string(), json))
                } else {
                    Ok(results
                        .into_iter()
                        .last()
                        .unwrap_or_else(|| StepOutput::new(String::new())))
                }
            }

            // ========== Suspend ==========
            StepAction::Suspend {
                reason,
                resume_schema,
                timeout_seconds: _timeout,
            } => {
                // Generate state token
                let state_token = uuid::Uuid::new_v4().to_string();

                // Save context via ContextStore if available
                if let Some(store) = &self.context_store {
                    let _ = store.save(ctx).await;
                }

                // Return suspended state in output
                Ok(StepOutput::new(format!(
                    "Suspended: {}. State token: {}. Resume schema: {}",
                    reason,
                    state_token,
                    resume_schema
                        .as_ref()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| "none".to_string())
                )))
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


