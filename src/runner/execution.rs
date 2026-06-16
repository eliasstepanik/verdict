use super::context_helpers::resolve_template;
use super::PipelineRunner;
use super::{PipelineError, OutputEvent};
use crate::action::{StepAction, StepError, StepOutput};
use crate::agent::{Agent, RemoteAgentClient, AgentPolicy};
use crate::audit::{AuditEntry, AuditEvent};
use crate::context::{StepContext, StepResult};
use crate::guards::{GuardEngine, GuardPhase};
use crate::pipeline::Pipeline;
use crate::skills::skill::SkillSet;
use crate::tools::ToolContext;
use crate::verdict::VerdictEngine;
use async_recursion::async_recursion;
use chrono::Utc;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use futures::StreamExt;

impl PipelineRunner {
    /// Topologically sort pipeline steps based on dependencies (Kahn's algorithm)
    /// Returns indices in execution order, or error if cycle is detected or dependency is missing.
    pub fn topological_sort(
        &self,
        pipeline: &Pipeline,
    ) -> Result<Vec<usize>, PipelineError> {
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

    /// Execute a tool call with full 8-step protocol
    pub(crate) async fn execute_tool_call(
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

        // Step 2.5: Track this tool as being used
        ctx.tools_used.push(tool_name.to_string());

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
    /// Execute a single step action — Phase 3 onwards
    #[async_recursion]
    pub async fn execute_action(
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
                let llm_client = self.llm_client.as_ref()
                    .ok_or_else(|| StepError::ActionFailed {
                        reason: "LLM client not configured".into(),
                    })?;

                // Resolve templates
                let resolved_system = resolve_template(system, ctx);
                let resolved_user = resolve_template(user, ctx);

                // Build the request
                let req = crate::llm::LlmRequest {
                    system: resolved_system,
                    user: resolved_user,
                    model: model
                        .as_ref()
                        .map(|ps| ps.model.clone())
                        .unwrap_or_else(|| llm_client.default_model().to_string()),
                    max_tokens: None,
                    history: conversation_id.as_ref().and_then(|id| {
                        let registry = self.conversation_registry.lock().ok()?;
                        registry.get(id).cloned()
                    }),
                    temperature: None,
                    tools: None,
                };

                // Call the LLM
                let response = llm_client.complete(req).await
                    .map_err(|e| StepError::ActionFailed {
                        reason: format!("LLM call failed: {}", e),
                    })?;

                // Increment budget
                ctx.budget.llm_calls_used += 1;

                // Save to conversation history if requested
                if *append_to_history {
                    if let Some(conv_id) = conversation_id {
                        if let Ok(mut registry) = self.conversation_registry.lock() {
                            let history = registry.get_or_create(conv_id);
                            history.push(
                                crate::llm::ChatRole::User,
                                resolve_template(user, ctx),
                            );
                            history.push(
                                crate::llm::ChatRole::Assistant,
                                response.content.clone(),
                            );
                        }
                    }
                }

                Ok(StepOutput::new(response.content))
            }

            // ========== LlmCallStreaming ==========
            StepAction::LlmCallStreaming {
                system,
                user,
                model,
                conversation_id,
                append_to_history,
            } => {
                let llm_client = self.llm_client.as_ref()
                    .ok_or_else(|| StepError::ActionFailed {
                        reason: "LLM client not configured".into(),
                    })?;

                // Resolve templates
                let resolved_system = resolve_template(system, ctx);
                let resolved_user = resolve_template(user, ctx);

                // Build the request
                let req = crate::llm::LlmRequest {
                    system: resolved_system,
                    user: resolved_user,
                    model: model
                        .as_ref()
                        .map(|ps| ps.model.clone())
                        .unwrap_or_else(|| llm_client.default_model().to_string()),
                    max_tokens: None,
                    history: conversation_id.as_ref().and_then(|id| {
                        let registry = self.conversation_registry.lock().ok()?;
                        registry.get(id).cloned()
                    }),
                    temperature: None,
                    tools: None,
                };

                // Stream the response
                let mut stream = llm_client.stream(req);
                let mut assembled = String::new();

                while let Some(chunk_result) = stream.next().await {
                    let chunk = chunk_result.map_err(|e| StepError::ActionFailed {
                        reason: format!("LLM streaming failed: {}", e),
                    })?;

                    assembled.push_str(&chunk.delta);

                    // Emit chunk to output sink
                    if let Some(sink) = &self.output_sink {
                        sink.emit(OutputEvent::LlmChunk {
                            step: ctx.step_name.clone(),
                            delta: chunk.delta,
                        })
                        .await;
                    }
                }

                // Increment budget
                ctx.budget.llm_calls_used += 1;

                // Save to conversation history if requested
                if *append_to_history {
                    if let Some(conv_id) = conversation_id {
                        if let Ok(mut registry) = self.conversation_registry.lock() {
                            let history = registry.get_or_create(conv_id);
                            history.push(
                                crate::llm::ChatRole::User,
                                resolve_template(user, ctx),
                            );
                            history.push(
                                crate::llm::ChatRole::Assistant,
                                assembled.clone(),
                            );
                        }
                    }
                }

                Ok(StepOutput::new(assembled))
            }

            // ========== ToolCall ==========
            StepAction::ToolCall { tool, args } => {
                // Emit ToolCallStarted to the real audit log before the call
                self.audit_log.append(AuditEntry {
                    timestamp: Utc::now(),
                    pipeline_name: ctx.pipeline_name.clone(),
                    step_name: ctx.step_name.clone(),
                    event: AuditEvent::ToolCallStarted {
                        tool: tool.clone(),
                        args: args.to_string(),
                    },
                });

                let result = self.execute_tool_call(tool, args, ctx).await;

                match &result {
                    Ok(output) => {
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: ctx.pipeline_name.clone(),
                            step_name: ctx.step_name.clone(),
                            event: AuditEvent::ToolCallCompleted {
                                tool: tool.clone(),
                                output_bytes: output.raw.len(),
                            },
                        });
                    }
                    Err(e) => {
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: ctx.pipeline_name.clone(),
                            step_name: ctx.step_name.clone(),
                            event: AuditEvent::ToolCallFailed {
                                tool: tool.clone(),
                                reason: e.to_string(),
                            },
                        });
                    }
                }

                result
            }

            // ========== SubPipeline ==========
            StepAction::SubPipeline(pipeline) => {
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
                };

                // Get the current agent (from context, fallback to a basic one)
                let agent = crate::agent::Agent {
                    name: ctx.agent_name.clone(),
                    description: "Child pipeline agent".into(),
                    pipeline: pipeline.as_ref().clone(),
                    tools: ctx.allowed_tools.clone(),
                    skills: SkillSet {
                        skills: ctx.active_skills.clone(),
                    },
                    policy: AgentPolicy::default(),
                };

                // Run the child pipeline
                let result = child_runner
                    .run(pipeline, &agent, ctx.input.clone())
                    .await
                    .map_err(|e| StepError::ActionFailed {
                        reason: format!("SubPipeline failed: {}", e),
                    })?;

                // Get the last step's output
                let output = result
                    .step_results
                    .values()
                    .last()
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
            StepAction::UseSkill { skill, input: _input, mode } => {
                let skill_def = self.skill_registry.get(skill).ok_or_else(|| {
                    StepError::ActionFailed {
                        reason: format!("Skill '{}' not found", skill),
                    }
                })?;

                // Check required guards
                for guard in &skill_def.required_guards {
                    GuardEngine::evaluate(guard, ctx).await.map_err(|e: crate::guards::GuardError| {
                        StepError::ActionFailed {
                            reason: format!("Skill required guard failed: {e}"),
                        }
                    })?;
                }

                match mode {
                    crate::action::SkillMode::PromptOnly => {
                        // If no LLM client, return the instructions directly
                        if self.llm_client.is_none() {
                            Ok(StepOutput::new(skill_def.instructions.clone()))
                        } else {
                            // Inject skill instructions into the system prompt of an LlmCall
                            let injected_system = format!(
                                "{}\n\n{}\n",
                                skill_def.instructions,
                                "{system}"
                            );
                            let injected_user = "{user}".to_string();

                            // We can't easily modify the action, so we'll build an LlmCall
                            let llm_call = StepAction::LlmCall {
                                system: injected_system,
                                user: injected_user,
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
                                let llm_call = StepAction::LlmCall {
                                    system: skill_def.instructions.clone(),
                                    user: String::new(),
                                    model: None,
                                    conversation_id: None,
                                    append_to_history: false,
                                };
                                self.execute_action(&llm_call, ctx).await
                            }
                        }
                    }
                }
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
                let llm_client = self.llm_client.as_ref()
                    .ok_or_else(|| StepError::ActionFailed {
                        reason: "LLM client not configured".into(),
                    })?;

                // Build tool schemas from registry
                let mut tool_schemas = Vec::new();
                for tool_name in tools {
                    if let Some(tool) = self.tool_registry.get(tool_name) {
                        tool_schemas.push(crate::llm::ToolSchema {
                            name: tool_name.clone(),
                            description: tool.description().to_string(),
                            parameters: tool.schema(),
                        });
                    }
                }

                let resolved_system = resolve_template(system, ctx);
                let resolved_user = resolve_template(user, ctx);

                let mut final_text = String::new();

                for round in 0..*max_rounds {
                    // Build request with accumulated history
                    let req = crate::llm::LlmRequest {
                        system: resolved_system.clone(),
                        user: if round == 0 {
                            resolved_user.clone()
                        } else {
                            final_text.clone()
                        },
                        model: model.model.clone(),
                        max_tokens: None,
                        history: None,
                        temperature: None,
                        tools: if tool_schemas.is_empty() {
                            None
                        } else {
                            Some(tool_schemas.clone())
                        },
                    };

                    // Call LLM
                    let response = llm_client.complete(req).await.map_err(|e| {
                        StepError::ActionFailed {
                            reason: format!("ToolUseLoop LLM call failed: {}", e),
                        }
                    })?;

                    final_text = response.content;
                    ctx.budget.llm_calls_used += 1;

                    // Check stop conditions
                    let should_stop = match stop_condition {
                        crate::action::StopCondition::TextOnly => {
                            response.tool_calls.is_none() || response.tool_calls.as_ref().map_or(true, |tc| tc.is_empty())
                        }
                        crate::action::StopCondition::Pattern(pattern) => {
                            final_text.contains(pattern)
                        }
                        crate::action::StopCondition::MaxRounds => round + 1 >= *max_rounds,
                    };

                    if should_stop {
                        break;
                    }

                    // Execute tool calls if present
                    if let Some(tool_calls) = response.tool_calls {
                        for tc in tool_calls {
                            if let Some(tool) = self.tool_registry.get(&tc.name) {
                                match tool.call(tc.arguments, ToolContext {
                                    audit_log: Arc::new(std::sync::Mutex::new(self.audit_log.clone())),
                                    filesystem_policy: ctx.filesystem_policy.clone(),
                                    network_policy: ctx.network_policy.clone(),
                                    allowed_tools: ctx.allowed_tools.clone(),
                                }).await {
                                    Ok(output) => {
                                        // Append tool result to the conversation
                                        final_text.push_str(&format!(
                                            "\nTool {} returned: {}",
                                            tc.name, output.raw
                                        ));
                                    }
                                    Err(e) => {
                                        final_text.push_str(&format!(
                                            "\nTool {} failed: {}",
                                            tc.name, e
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(StepOutput::new(final_text))
            }

            // ========== Custom ==========
            StepAction::Custom(func) => func(ctx),

            // ========== UserInput ==========
            StepAction::UserInput { prompt, schema: _ } => {
                eprintln!("{} [y/N]: ", prompt);
                let stdin = std::io::stdin();
                let mut line = String::new();
                stdin.read_line(&mut line).map_err(|e| {
                    StepError::ActionFailed {
                        reason: format!("Failed to read user input: {}", e),
                    }
                })?;

                Ok(StepOutput::new(line.trim().to_string()))
            }

            // ========== DelegateAgent ==========
            // NOTE: DelegateAgent should be handled in the main run() method before execute_action.
            // If we reach here, it means the caller misused the runner.
            StepAction::DelegateAgent { .. } => {
                Err(StepError::ActionFailed {
                    reason: "DelegateAgent should be handled by PipelineRunner::run(), not execute_action()".into(),
                })
            }
        }
    }

    /// Execute a pipeline with a specified delegation depth and parent agent
    /// This is the internal entry point used for recursive delegation.
    #[async_recursion]
    pub async fn run_with_delegation_depth(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
        delegation_depth: u32,
        parent_agent: String,
    ) -> Result<super::PipelineResult, PipelineError> {
        // Run the pipeline with delegation context injected
        self.run_internal(pipeline, agent, input, Some((delegation_depth, parent_agent))).await
    }

    /// Execute a pipeline with an agent
    #[async_recursion]
    pub async fn run(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
    ) -> Result<super::PipelineResult, PipelineError> {
        self.run_internal(pipeline, agent, input, None).await
    }

    /// Internal pipeline runner with optional delegation context
    #[async_recursion]
    async fn run_internal(
        &mut self,
        pipeline: &Pipeline,
        agent: &Agent,
        input: Value,
        delegation_context: Option<(u32, String)>, // (delegation_depth, parent_agent)
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
        ctx.agent_registry = self.agent_registry.clone();
        ctx.tool_registry = self.tool_registry.clone();
        ctx.skill_registry = self.skill_registry.clone();
        ctx.llm_client = self.llm_client.clone();

        // Apply delegation context if provided
        if let Some((depth, parent)) = delegation_context {
            ctx.delegation_depth = depth;
            ctx.parent_agent = Some(parent);
        }

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
                
                // Execute batch in parallel using spawn_blocking for each step
                let mut handles = Vec::new();
                
                for &sidx in &batch {
                    let step = pipeline.steps[sidx].clone();
                    let mut step_ctx = ctx.clone();
                    step_ctx.step_name = step.name.clone();
                    step_ctx.input = input.clone();
                    step_ctx.allowed_tools = crate::toolset::ToolSet::Intersection(
                        Box::new(agent.policy.allowed_tools.clone()),
                        Box::new(step.tools.clone()),
                    );
                    
                    // Clone the action needed for the task
                    let action = step.action.clone();
                    
                    let handle = tokio::task::spawn_blocking(move || {
                        // Run the action synchronously in a thread pool
                        // For Custom closures (which are blocking), call directly
                        match &action {
                            crate::action::StepAction::Custom(f) => f(&step_ctx),
                            _ => Err(crate::action::StepError::ActionFailed {
                                reason: "Only Custom actions supported in parallel steps".into(),
                            }),
                        }
                    });
                    
                    handles.push((step.name.clone(), step.clone(), handle));
                }
                
                // Collect results and process them sequentially
                for (step_name, step_obj, handle) in handles {
                    let action_result = handle.await.map_err(|e| PipelineError::StepFailed {
                        step: step_name.clone(),
                        error: crate::action::StepError::ActionFailed { reason: e.to_string() },
                    })?;
                    
                    match action_result {
                        Ok(output) => {
                            // Run guard_out and verdict for this parallel step
                            ctx.step_name = step_name.clone();
                            ctx.output = Some(output);
                            
                            // guard_out
                            match GuardEngine::evaluate(&step_obj.guard_out, &ctx).await {
                                Ok(()) => {}
                                Err(e) => {
                                    return Err(PipelineError::GuardFailed {
                                        step: step_name.clone(),
                                        phase: GuardPhase::Out,
                                        error: e,
                                    });
                                }
                            }
                            
                            // verdict
                            match VerdictEngine::evaluate(&step_obj.verdict, &ctx).await {
                                Ok(()) => {}
                                Err(e) => {
                                    return Err(PipelineError::VerdictFailed {
                                        step: step_name.clone(),
                                        error: e,
                                    });
                                }
                            }
                            
                            // Record result
                            let sr = StepResult {
                                step_name: step_name.clone(),
                                output: ctx.output.clone().unwrap_or_else(|| StepOutput::new(String::new())),
                                verdict_passed: true,
                                error: None,
                            };
                            ctx.step_results.insert(step_name.clone(), sr);
                            steps_passed.push(step_name);
                        }
                        Err(e) => {
                            return Err(PipelineError::StepFailed {
                                step: step_name,
                                error: e,
                            });
                        }
                    }
                }
                continue;
            } else {
                i += 1;
            }
            
            let step = &pipeline.steps[step_idx].clone();

            ctx.step_name = step.name.clone();
            ctx.input = input.clone();
            
            // Compute effective tool scope for this step
            ctx.allowed_tools = crate::toolset::ToolSet::Intersection(
                Box::new(agent.policy.allowed_tools.clone()),
                Box::new(step.tools.clone()),
            );

            // ===== Record StepStarted audit event =====
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: pipeline.name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::StepStarted,
            });

            // Handle guard_in
            match GuardEngine::evaluate(&step.guard_in, &ctx).await {
                Ok(()) => {
                    // ===== Record GuardPassed(in) audit event =====
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardPassed { guard: step.guard_in.name() },
                    });
                }
                Err(e) => {
                    let guard_err: crate::guards::GuardError = e;
                    let err_str = format!("{guard_err}");
                    // Record GuardFailed audit event
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardFailed {
                            guard: step.guard_in.name(),
                            reason: err_str.clone(),
                        },
                    });

                    // Handle guard_in failure based on FailureMode
                    match &pipeline.on_failure {
                        crate::pipeline::FailureMode::Skip => {
                            // Skip this step, continue to next
                            steps_failed.push(step.name.clone());
                            let sr = StepResult {
                                step_name: step.name.clone(),
                                output: StepOutput::new(String::new()),
                                verdict_passed: false,
                                error: Some(format!("guard_in failed: {err_str}")),
                            };
                            ctx.step_results.insert(step.name.clone(), sr);

                            // Record StepFailed audit event
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
                        if retries_left > 0 && matches!(&pipeline.on_failure, crate::pipeline::FailureMode::Retry) {
                            retries_left -= 1;
                            // Continue loop to retry
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
                        // Skip this step, continue to next
                        steps_failed.push(step.name.clone());
                        let sr = StepResult {
                            step_name: step.name.clone(),
                            output: ctx.output.clone().unwrap_or_else(|| StepOutput::new(String::new())),
                            verdict_passed: false,
                            error: action_error.as_ref().map(|e| format!("{:?}", e)),
                        };
                        ctx.step_results.insert(step.name.clone(), sr);
                        
                        // Record StepFailed audit event
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
                        // Max retries exceeded
                        return Err(PipelineError::MaxRetriesExceeded {
                            step: step.name.clone(),
                        });
                    }
                    crate::pipeline::FailureMode::Abort => {
                        // Record StepFailed audit event before aborting
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
                        
                        // Log fallback triggered
                        self.audit_log.append(AuditEntry {
                            timestamp: Utc::now(),
                            pipeline_name: pipeline.name.clone(),
                            step_name: step.name.clone(),
                            event: AuditEvent::FallbackTriggered {
                                step: step.name.clone(),
                                reason: format!("{:?}", original_error),
                            },
                        });
                        
                        // Run the fallback pipeline in a child runner
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
                        };
                        
                        return fallback_runner
                            .run(fallback_pipeline, agent, input.clone())
                            .await
                            .map_err(|_| PipelineError::StepFailed {
                                step: step_name_clone,
                                error: original_error,
                            });
                    }
                }
            }

            // Handle guard_out (only if action succeeded)
            match GuardEngine::evaluate(&step.guard_out, &ctx).await {
                Ok(()) => {
                    // ===== Record GuardPassed(out) audit event =====
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardPassed { guard: step.guard_out.name() },
                    });
                }
                Err(e) => {
                    let guard_err: crate::guards::GuardError = e;
                    // Record GuardFailed audit event
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::GuardFailed {
                            guard: step.guard_out.name(),
                            reason: format!("{guard_err}"),
                        },
                    });
                    return Err(PipelineError::GuardFailed {
                        step: step.name.clone(),
                        phase: GuardPhase::Out,
                        error: guard_err,
                    });
                }
            }

            // Handle verdict
            match VerdictEngine::evaluate(&step.verdict, &ctx).await {
                Ok(()) => {
                    // ===== Record VerdictPassed audit event =====
                    self.audit_log.append(AuditEntry {
                        timestamp: Utc::now(),
                        pipeline_name: pipeline.name.clone(),
                        step_name: step.name.clone(),
                        event: AuditEvent::VerdictPassed { verdict: "verdict".into() },
                    });
                }
                Err(e) => {
                    return Err(PipelineError::VerdictFailed {
                        step: step.name.clone(),
                        error: e,
                    });
                }
            }

            // ===== Record StepCompleted audit event =====
            self.audit_log.append(AuditEntry {
                timestamp: Utc::now(),
                pipeline_name: pipeline.name.clone(),
                step_name: step.name.clone(),
                event: AuditEvent::StepCompleted { verdict_passed: true },
            });

            // Record step result
            let sr = StepResult {
                step_name: step.name.clone(),
                output: ctx.output.clone().unwrap_or_else(|| StepOutput::new(String::new())),
                verdict_passed: true,
                error: None,
            };
            ctx.step_results.insert(step.name.clone(), sr);
            steps_passed.push(step.name.clone());

            // Auto-save after each step
            if let Some(store) = &self.context_store {
                if let Err(e) = store.save(&ctx).await {
                    eprintln!("[verdict] warning: ContextStore::save failed: {e}");
                }
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
        })
    }
}
