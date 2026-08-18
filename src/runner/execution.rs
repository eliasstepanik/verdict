use super::PipelineRunner;
use super::{OutputEvent, PipelineError};
use crate::action::{StepAction, StepError, StepOutput};
use crate::agent::{Agent, AgentPolicy, RemoteAgentClient};
use crate::audit::{AuditEntry, AuditEvent};
use crate::context::{StepContext, StepResult};
use crate::guards::{GuardEngine, GuardPhase};
use crate::pipeline::Pipeline;
use crate::skills::skill::SkillSet;
use crate::tools::ToolContext;
use async_recursion::async_recursion;
use chrono::Utc;
use futures::StreamExt;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use tempfile;

/// Resolve template placeholders in a prompt string.
///
/// Supported placeholders:
/// - `{input}` → the pipeline input value (as string)
/// - `{step_name}` → the raw output of the named prior step
///
/// WARNING: Template substitution uses sequential string replacement. If a step output
/// contains placeholder-like patterns (e.g., `{next_step_name}`), those patterns may be
/// substituted in subsequent iterations, potentially causing unexpected cascading replacements.
/// For safety, avoid step outputs that contain literal `{...}` patterns matching step names.
fn resolve_template(template: &str, ctx: &StepContext) -> String {
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
        let value_str = step_result.output.raw.clone();
        result = result.replace(&format!("{{{}}}", step_name), &value_str);
    }

    result
}

/// Strip XML tool-call artifacts that Claude sometimes halluccinates in synthesis responses.
/// Removes `<function_calls>...</function_calls>`, `<function_response>...</function_response>`, and `<invoke>...</invoke>` blocks.
fn strip_xml_tool_calls(text: &str) -> String {
    let mut result = text.to_string();
    // Remove <function_calls>...</function_calls> blocks (greedy, handles multiline)
    while let (Some(start), Some(end)) = (
        result.find("<function_calls>"),
        result.find("</function_calls>"),
    ) {
        if start <= end {
            result = format!(
                "{}{}",
                &result[..start],
                &result[end + "</function_calls>".len()..]
            );
        } else {
            break;
        }
    }
    // Remove <function_response>...</function_response> blocks
    while let (Some(start), Some(end)) = (
        result.find("<function_response>"),
        result.find("</function_response>"),
    ) {
        if start <= end {
            result = format!(
                "{}{}",
                &result[..start],
                &result[end + "</function_response>".len()..]
            );
        } else {
            break;
        }
    }
    // Remove <invoke>...</invoke> blocks (standalone XML tool calls)
    while let (Some(start), Some(end)) = (result.find("<invoke"), result.rfind("</invoke>")) {
        if start < end {
            result = format!("{}{}", &result[..start], &result[end + "</invoke>".len()..]);
        } else {
            break;
        }
    }
    // Collapse multiple blank lines
    let re_multi_blank = result
        .split('\n')
        .fold((String::new(), 0usize), |(mut acc, blanks), line| {
            if line.trim().is_empty() {
                if blanks < 1 {
                    acc.push('\n');
                }
                (acc, blanks + 1)
            } else {
                acc.push_str(line);
                acc.push('\n');
                (acc, 0)
            }
        })
        .0;
    re_multi_blank.trim().to_string()
}

/// Parse XML-format tool calls from text (Claude's legacy XML format).
///
/// Extracts all `<invoke name="TOOL_NAME">...</invoke>` blocks and converts them to (tool_name, args_json) pairs.
/// Each block should contain `<parameter name="KEY">VALUE</parameter>` entries.
///
/// Returns a Vec of (tool_name, args_json) pairs, or an empty Vec if no XML tool calls are found.
fn parse_xml_tool_calls(text: &str) -> Vec<(String, serde_json::Value)> {
    let mut result = Vec::new();

    // Find all <invoke name="..."> blocks
    let mut remaining = text;
    while let Some(start) = remaining.find("<invoke") {
        // Look for the closing > of the opening tag
        if let Some(tag_end) = remaining[start..].find('>') {
            let tag_end = start + tag_end;

            // Extract the opening tag to get the tool name
            let open_tag = &remaining[start..=tag_end];
            if let Some(name_start) = open_tag.find("name=\"") {
                let name_start = start + name_start + 6; // len("name=\"")
                if let Some(name_end) = remaining[name_start..].find('"') {
                    let tool_name = remaining[name_start..name_start + name_end].to_string();

                    // Find the closing </invoke> tag
                    if let Some(close_pos) = remaining[tag_end + 1..].find("</invoke>") {
                        let close_pos = tag_end + 1 + close_pos;
                        let block_content = &remaining[tag_end + 1..close_pos];

                        // Parse <parameter name="KEY">VALUE</parameter> entries
                        let mut args = serde_json::json!({});
                        let mut param_remaining = block_content;

                        while let Some(param_start) = param_remaining.find("<parameter") {
                            if let Some(param_tag_end) = param_remaining[param_start..].find('>') {
                                let param_tag_end = param_start + param_tag_end;
                                let param_tag = &param_remaining[param_start..=param_tag_end];

                                // Extract parameter name
                                if let Some(pname_start) = param_tag.find("name=\"") {
                                    let pname_start = param_start + pname_start + 6;
                                    if let Some(pname_end) =
                                        param_remaining[pname_start..].find('"')
                                    {
                                        let param_name = param_remaining
                                            [pname_start..pname_start + pname_end]
                                            .to_string();

                                        // Find the closing </parameter> tag
                                        if let Some(pclose_pos) = param_remaining
                                            [param_tag_end + 1..]
                                            .find("</parameter>")
                                        {
                                            let pclose_pos = param_tag_end + 1 + pclose_pos;
                                            let param_value = param_remaining
                                                [param_tag_end + 1..pclose_pos]
                                                .trim()
                                                .to_string();

                                            // Try to parse as JSON, otherwise treat as string
                                            if let Ok(json_val) = serde_json::from_str(&param_value)
                                            {
                                                args[&param_name] = json_val;
                                            } else {
                                                args[&param_name] =
                                                    serde_json::Value::String(param_value);
                                            }

                                            param_remaining = &param_remaining
                                                [pclose_pos + "</parameter>".len()..];
                                        } else {
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }

                        result.push((tool_name, args));
                        remaining = &remaining[close_pos + "</invoke>".len()..];
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    result
}

/// Extract the actual shell command string from tool args.
/// For shell.* tools, builds a command string that combines the command + args.
/// For tools like shell.cargo_test, shell.cargo_check, returns just the command name.
fn extract_shell_command_string(tool_name: &str, args: &Value) -> Result<String, String> {
    match tool_name {
        "shell.run" | "shell.run_command" => {
            // Both shell.run and shell.run_command have {"command": "...", "args": ["...", ...]}
            if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
                let cmd_args: Vec<String> = args
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                
                if cmd_args.is_empty() {
                    Ok(cmd.to_string())
                } else {
                    Ok(format!("{} {}", cmd, cmd_args.join(" ")))
                }
            } else {
                Err("missing 'command' field".to_string())
            }
        }
        "shell.cargo_test" => Ok("cargo test".to_string()),
        "shell.cargo_check" => Ok("cargo check".to_string()),
        "shell.cargo_build" => Ok("cargo build".to_string()),
        _ => {
            // For other shell.* tools, try to extract a reasonable command string
            // Fall back to the tool name without "shell." prefix
            Ok(tool_name.strip_prefix("shell.").unwrap_or(tool_name).to_string())
        }
    }
}

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
        if !ctx.allowed_tools.contains_with_skill_registry(tool_name, &self.skill_registry) {
            return Err(StepError::ActionFailed {
                reason: format!(
                    "tool '{}' not allowed in this step (allowed: {:?})",
                    tool_name, ctx.allowed_tools
                ),
            });
        }

        // Step 2.5: Track this tool as being used
        ctx.tools_used.push(tool_name.to_string());

        // Step 2.6: For shell tools, extract and record the actual command
        if tool_name.starts_with("shell.") {
            if let Ok(cmd_str) = extract_shell_command_string(tool_name, &args) {
                ctx.commands_executed.push((tool_name.to_string(), cmd_str));
            }
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
                self.record_tool_call_cost(ctx);

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
                let llm_client =
                    self.llm_client
                        .as_ref()
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
                    tool_choice: None,
                };

                // Call the LLM
                let response =
                    llm_client
                        .complete(req)
                        .await
                        .map_err(|e| StepError::ActionFailed {
                            reason: format!("LLM call failed: {}", e),
                        })?;

                // Increment budget
                ctx.budget.llm_calls_used += 1;

                // Record cost if usage info is available
                if let Some(usage) = &response.usage {
                    self.record_llm_cost(usage, &response.model, ctx);
                }

                // Save to conversation history if requested
                if *append_to_history {
                    if let Some(conv_id) = conversation_id {
                        if let Ok(mut registry) = self.conversation_registry.lock() {
                            let history = registry.get_or_create(conv_id);
                            history.push(crate::llm::ChatRole::User, resolve_template(user, ctx));
                            history.push(crate::llm::ChatRole::Assistant, response.content.clone());
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
                let llm_client =
                    self.llm_client
                        .as_ref()
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
                    tool_choice: None,
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

                // NOTE: LlmCallStreaming does not track cost (record_llm_cost requires usage stats
                // from the API response, but streaming returns chunks without aggregated usage info).
                // To instrument streaming cost, the LLM provider would need to return usage stats
                // at stream end (not yet implemented in our LlmChunk structure).

                // Save to conversation history if requested
                if *append_to_history {
                    if let Some(conv_id) = conversation_id {
                        if let Ok(mut registry) = self.conversation_registry.lock() {
                            let history = registry.get_or_create(conv_id);
                            history.push(crate::llm::ChatRole::User, resolve_template(user, ctx));
                            history.push(crate::llm::ChatRole::Assistant, assembled.clone());
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
                    auto_title_llm: self.auto_title_llm.clone(),
                    memory: self.memory.clone(),
                };

                // Get the current agent (from context, fallback to a basic one)
                // FIX #3: Propagate narrowed tool scope from parent context into sub-agent's policy
                // so that the effective tool scope for steps inside the sub-pipeline is correctly:
                // parent_narrowed_scope ∩ step.tools (not default ∩ step.tools which would be too restrictive)
                let mut policy = AgentPolicy::default();
                policy.allowed_tools = ctx.allowed_tools.clone();
                
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
                            // FIX #2: Inject skill instructions as system prompt only.
                            // Do NOT include literal "{system}" or "{user}" placeholders —
                            // the template engine only substitutes {input} and {step_name},
                            // so these would leak into the LLM call as garbage text.
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
                };

                // Restore original tool scope after skill execution
                ctx.allowed_tools = saved_tools;
                result
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
                let llm_client =
                    self.llm_client
                        .as_ref()
                        .ok_or_else(|| StepError::ActionFailed {
                            reason: "LLM client not configured".into(),
                        })?;

                // Build tool schemas from registry.
                // Anthropic (and many other providers) reject tool names containing dots —
                // the API enforces ^[a-zA-Z0-9_-]{1,128}$. We sanitize dots → underscores
                // for the LLM-facing name and keep a reverse map to restore the registry key.
                let mut tool_schemas = Vec::new();
                let mut tool_name_map: std::collections::HashMap<String, String> =
                    std::collections::HashMap::new();
                for tool_name in tools {
                    if let Some(tool) = self.tool_registry.get(tool_name) {
                        let safe_name = tool_name.replace('.', "_");
                        tool_name_map.insert(safe_name.clone(), tool_name.clone());
                        tool_schemas.push(crate::llm::ToolSchema {
                            name: safe_name,
                            description: tool.description().to_string(),
                            parameters: tool.schema(),
                        });
                    }
                }

                let resolved_system = resolve_template(system, ctx);
                let resolved_user = resolve_template(user, ctx);

                eprintln!(
                    "[toolloop-init] system_len={} user_len={} user_preview={}",
                    resolved_system.len(),
                    resolved_user.len(),
                    &resolved_user[..resolved_user.len().min(80)]
                );

                // Conversation history accumulated across rounds.
                // Each round: send history + new user/tool messages, get assistant response.
                // If assistant returns tool_calls: execute them, add results as tool messages, loop.
                // If assistant returns text: done.
                let mut history = crate::llm::MessageHistory::new();
                let mut final_text = String::new();
                // True when the loop ended because the assistant produced a real text answer
                // with no pending tool calls — i.e. the answer is already final.
                let mut answered_with_text = false;

                for round in 0..*max_rounds {
                    // On round 0: user message is the resolved prompt.
                    // On subsequent rounds: history already has the prior assistant + tool result
                    // messages. We do NOT add an extra user message — the API expects us to
                    // continue the conversation from the tool results. The provider skips
                    // empty user messages so the messages array ends with tool-role entries,
                    // which is valid for OpenAI/Anthropic function-calling APIs.
                    let user_msg = if round == 0 {
                        resolved_user.clone()
                    } else {
                        // History already contains tool results; no user message needed.
                        String::new()
                    };

                    // Determine tool_choice: "auto" if tools available, None if no tools.
                    // Proxy ignores tool_choice anyway; instruction in system prompt enforces tool use.
                    let tool_choice = if !tool_schemas.is_empty() {
                        Some("auto".to_string())
                    } else {
                        None
                    };

                    // Belt and suspenders: append instruction to system prompt when tools are required on round 0
                    // This ensures the model prioritizes tool calls even if tool_choice conversion fails
                    let effective_system = if round == 0 && !tool_schemas.is_empty() {
                        format!("{}\n\nWICHTIG: Du MUSST jetzt ein Tool aufrufen. Antworte NICHT mit Text — rufe sofort das passende Tool auf.", resolved_system)
                    } else {
                        resolved_system.clone()
                    };

                    let req = crate::llm::LlmRequest {
                        system: effective_system.clone(),
                        user: user_msg.clone(),
                        model: if model.model.is_empty() {
                            llm_client.default_model().to_string()
                        } else {
                            model.model.clone()
                        },
                        max_tokens: None,
                        history: if history.is_empty() {
                            None
                        } else {
                            Some(history.clone())
                        },
                        temperature: None,
                        tools: if tool_schemas.is_empty() {
                            None
                        } else {
                            Some(tool_schemas.clone())
                        },
                        tool_choice,
                    };

                    let response =
                        llm_client
                            .complete(req)
                            .await
                            .map_err(|e| StepError::ActionFailed {
                                reason: format!("ToolUseLoop LLM call failed: {}", e),
                            })?;

                    ctx.budget.llm_calls_used += 1;

                    // Record cost if usage info is available
                    if let Some(usage) = &response.usage {
                        self.record_llm_cost(usage, &response.model, ctx);
                    }

                    final_text = response.content.clone();
                    let mut has_tool_calls = response
                        .tool_calls
                        .as_ref()
                        .map_or(false, |tc| !tc.is_empty());

                    // Check for XML-format tool calls if JSON tool_calls are absent
                    let xml_tool_calls = if !has_tool_calls {
                        parse_xml_tool_calls(&final_text)
                    } else {
                        Vec::new()
                    };

                    let should_stop = match stop_condition {
                        crate::action::StopCondition::TextOnly => {
                            !has_tool_calls && xml_tool_calls.is_empty()
                        }
                        crate::action::StopCondition::Pattern(pattern) => {
                            final_text.contains(pattern)
                        }
                        crate::action::StopCondition::MaxRounds => round + 1 >= *max_rounds,
                    };

                    if should_stop {
                        // Add this exchange to history before stopping (for context)
                        if !user_msg.is_empty() {
                            history.push(crate::llm::ChatRole::User, user_msg);
                        }
                        history.push(crate::llm::ChatRole::Assistant, final_text.clone());
                        // The assistant answered in prose with nothing left to execute:
                        // this text IS the result, so no synthesis pass is needed.
                        answered_with_text = !has_tool_calls
                            && xml_tool_calls.is_empty()
                            && !final_text.trim().is_empty();
                        break;
                    }

                    // Add user message to history (round 0 only — subsequent rounds have tool msgs)
                    if !user_msg.is_empty() {
                        history.push(crate::llm::ChatRole::User, user_msg);
                    }

                    // Add assistant message with proper tool_calls JSON for the API
                    // When tool_calls are present, content may be empty — that's fine
                    if let Some(ref tool_calls_list) = response.tool_calls {
                        // Build the tool_calls array in OpenAI format
                        let tool_calls_json = serde_json::json!(tool_calls_list
                            .iter()
                            .enumerate()
                            .map(|(i, tc)| {
                                let call_id =
                                    tc.id.clone().unwrap_or_else(|| format!("call_{}", i));
                                serde_json::json!({
                                    "id": call_id,
                                    "type": "function",
                                    "function": {
                                        "name": tc.name,
                                        "arguments": tc.arguments.to_string()
                                    }
                                })
                            })
                            .collect::<Vec<_>>());
                        history
                            .messages
                            .push(crate::llm::ChatMessage::assistant_with_tool_calls(
                                final_text.clone(),
                                tool_calls_json,
                            ));

                        // Execute each tool and add its result with proper tool_call_id
                        for (i, tc) in tool_calls_list.iter().enumerate() {
                            let call_id = tc.id.clone().unwrap_or_else(|| format!("call_{}", i));
                            let registry_name = tool_name_map
                                .get(&tc.name)
                                .cloned()
                                .unwrap_or_else(|| tc.name.clone());

                            eprintln!(
                                "[tool-call] llm_name={} registry_name={} args={}",
                                tc.name, registry_name, tc.arguments
                            );

                            let tool_result =
                                if let Some(tool) = self.tool_registry.get(&registry_name) {
                                    match tool
                                        .call(
                                            tc.arguments.clone(),
                                            ToolContext {
                                                audit_log: Arc::new(std::sync::Mutex::new(
                                                    self.audit_log.clone(),
                                                )),
                                                filesystem_policy: ctx.filesystem_policy.clone(),
                                                network_policy: ctx.network_policy.clone(),
                                                allowed_tools: ctx.allowed_tools.clone(),
                                            },
                                        )
                                        .await
                                    {
                                        Ok(output) => {
                                            let pe = output
                                                .raw
                                                .char_indices()
                                                .nth(80)
                                                .map(|(i, _)| i)
                                                .unwrap_or(output.raw.len());
                                            eprintln!(
                                                "[tool-ok] {}: {}",
                                                registry_name,
                                                &output.raw[..pe]
                                            );
                                            output.raw
                                        }
                                        Err(e) => {
                                            eprintln!("[tool-err] {}: {}", registry_name, e);
                                            format!("Tool error: {}", e)
                                        }
                                    }
                                } else {
                                    eprintln!(
                                    "[tool-notfound] '{}' (from llm: '{}'). available in map: {:?}",
                                    registry_name,
                                    tc.name,
                                    tool_name_map.keys().collect::<Vec<_>>()
                                );
                                    format!("Tool '{}' not found", registry_name)
                                };

                            history
                                .messages
                                .push(crate::llm::ChatMessage::tool_result(call_id, tool_result));
                        }
                    } else if !xml_tool_calls.is_empty() {
                        // XML tool calls found — execute them
                        has_tool_calls = true;

                        // FIX: build tool_calls JSON array with synthetic IDs matching what tool_result will use
                        let tool_calls_json = serde_json::json!(xml_tool_calls
                            .iter()
                            .enumerate()
                            .map(|(i, (tool_name, args))| {
                                serde_json::json!({
                                    "id": format!("xml_call_{}", i),
                                    "type": "function",
                                    "function": {
                                        "name": tool_name,
                                        "arguments": args.to_string()
                                    }
                                })
                            })
                            .collect::<Vec<_>>());
                        // Push assistant message WITH tool_calls so IDs are in the message history
                        history
                            .messages
                            .push(crate::llm::ChatMessage::assistant_with_tool_calls(
                                final_text.clone(),
                                tool_calls_json,
                            ));

                        // Execute each XML tool call
                        for (i, (tool_name, args)) in xml_tool_calls.iter().enumerate() {
                            let call_id = format!("xml_call_{}", i);

                            eprintln!("[xml-tool-call] tool_name={} args={}", tool_name, args);

                            let tool_result = if let Some(tool) = self.tool_registry.get(tool_name)
                            {
                                match tool
                                    .call(
                                        args.clone(),
                                        ToolContext {
                                            audit_log: Arc::new(std::sync::Mutex::new(
                                                self.audit_log.clone(),
                                            )),
                                            filesystem_policy: ctx.filesystem_policy.clone(),
                                            network_policy: ctx.network_policy.clone(),
                                            allowed_tools: ctx.allowed_tools.clone(),
                                        },
                                    )
                                    .await
                                {
                                    Ok(output) => {
                                        eprintln!(
                                            "[xml-tool-ok] {}: {}",
                                            tool_name,
                                            &output.raw[..output.raw.len().min(80)]
                                        );
                                        output.raw
                                    }
                                    Err(e) => {
                                        eprintln!("[xml-tool-err] {}: {}", tool_name, e);
                                        format!("Tool error: {}", e)
                                    }
                                }
                            } else {
                                eprintln!("[xml-tool-notfound] '{}'", tool_name);
                                format!("Tool '{}' not found", tool_name)
                            };

                            history
                                .messages
                                .push(crate::llm::ChatMessage::tool_result(call_id, tool_result));
                        }
                    } else {
                        // No tool calls — plain assistant message
                        history.push(crate::llm::ChatRole::Assistant, final_text.clone());
                    }
                }

                // If we exhausted rounds, final text is empty, or XML tool calls were executed,
                // enter a synthesis loop: call LLM with tool schemas enabled so it can make
                // XML tool calls, execute them, feed results back, and repeat until LLM
                // produces text-only output (no more XML tool calls). Cap at 10 synthesis rounds.
                // Always run synthesis when tools are available — the LLM may have responded
                // with a text preamble on round 0 without using tools; synthesis gives it a
                // chance to actually call tools and complete the task.
                // ponytail: synthesis exists only to rescue an unfinished loop. If the model
                // already delivered a final text answer, another LLM call would overwrite it.
                let needs_synthesis =
                    !history.is_empty() && !tool_schemas.is_empty() && !answered_with_text;
                if needs_synthesis {
                    // Build tool list for XML instruction so model knows what tools are available
                    let tool_list = tool_schemas
                        .iter()
                        .map(|t| format!("  - {} : {}", t.name, t.description))
                        .collect::<Vec<_>>()
                        .join("\n");

                    let xml_instruction = format!(
                        "Please complete the task now. Available tools:\n{}\n\nCall ONE tool at a time using XML:\n<invoke name=\"TOOL_NAME\"><parameter name=\"KEY\">VALUE</parameter></invoke>\n\nWait for each tool result before calling the next tool, especially when you need the ID from one call to use in the next.\n\nAfter all tool calls, provide your final text answer.",
                        tool_list
                    );

                    // Add explicit user turn so conversation is valid
                    history.push(crate::llm::ChatRole::User, xml_instruction);

                    let mut consecutive_tool_failures = 0;
                    for _syn_round in 0..10 {
                        // Send synthesis WITHOUT tool schemas so the API doesn't enforce
                        // structured tool calling. This lets Claude use its XML tool-call
                        // format (<invoke name="...">...</invoke>) freely in its text response,
                        // which we then parse and execute via parse_xml_tool_calls().
                        let has_tool_results = history
                            .messages
                            .iter()
                            .any(|m| matches!(m.role, crate::llm::ChatRole::Tool));
                        let synthesis_req = crate::llm::LlmRequest {
                            system: resolved_system.clone(),
                            user: String::new(),
                            model: if model.model.is_empty() {
                                llm_client.default_model().to_string()
                            } else {
                                model.model.clone()
                            },
                            max_tokens: None,
                            history: Some(history.clone()),
                            temperature: None,
                            // No API tools — model uses XML <invoke> format freely (parsed by parse_xml_tool_calls)
                            tools: None,
                            tool_choice: None,
                        };
                        let _ = has_tool_results; // used for logging only if needed

                        match llm_client.complete(synthesis_req).await {
                            Ok(syn_resp) => {
                                ctx.budget.llm_calls_used += 1;

                                // Record cost if usage info is available
                                if let Some(usage) = &syn_resp.usage {
                                    self.record_llm_cost(usage, &syn_resp.model, ctx);
                                }

                                let raw = syn_resp.content.clone();

                                // Handle JSON tool calls in synthesis response
                                if let Some(ref tcs) = syn_resp.tool_calls {
                                    if !tcs.is_empty() {
                                        let tc_json = serde_json::json!(
                                            tcs.iter().enumerate().map(|(i, tc)| {
                                                let cid = tc.id.clone().unwrap_or_else(|| format!("syn_call_{}", i));
                                                serde_json::json!({"id": cid, "type": "function", "function": {"name": tc.name, "arguments": tc.arguments.to_string()}})
                                            }).collect::<Vec<_>>()
                                        );
                                        history.messages.push(
                                            crate::llm::ChatMessage::assistant_with_tool_calls(
                                                raw.clone(),
                                                tc_json,
                                            ),
                                        );
                                        let mut tool_results = Vec::new();
                                        for (i, tc) in tcs.iter().enumerate() {
                                            let cid = tc
                                                .id
                                                .clone()
                                                .unwrap_or_else(|| format!("syn_call_{}", i));
                                            let rname = tool_name_map
                                                .get(&tc.name)
                                                .cloned()
                                                .unwrap_or_else(|| tc.name.clone());
                                            let result = if let Some(tool) =
                                                self.tool_registry.get(&rname)
                                            {
                                                match tool
                                                    .call(
                                                        tc.arguments.clone(),
                                                        ToolContext {
                                                            audit_log: Arc::new(
                                                                std::sync::Mutex::new(
                                                                    self.audit_log.clone(),
                                                                ),
                                                            ),
                                                            filesystem_policy: ctx
                                                                .filesystem_policy
                                                                .clone(),
                                                            network_policy: ctx
                                                                .network_policy
                                                                .clone(),
                                                            allowed_tools: ctx
                                                                .allowed_tools
                                                                .clone(),
                                                        },
                                                    )
                                                    .await
                                                {
                                                    Ok(o) => {
                                                        eprintln!("[syn-tool-ok] {}", rname);
                                                        o.raw
                                                    }
                                                    Err(e) => {
                                                        eprintln!(
                                                            "[syn-tool-err] {}: {}",
                                                            rname, e
                                                        );
                                                        format!("Tool error: {}", e)
                                                    }
                                                }
                                            } else {
                                                format!("Tool '{}' not found", rname)
                                            };
                                            tool_results.push(result.clone());
                                            history.messages.push(
                                                crate::llm::ChatMessage::tool_result(cid, result),
                                            );
                                        }

                                        // Track consecutive tool failures
                                        let all_failed = tool_results
                                            .iter()
                                            .all(|r| r.starts_with("Tool error:"));
                                        if all_failed && !tool_results.is_empty() {
                                            consecutive_tool_failures += 1;
                                        } else {
                                            consecutive_tool_failures = 0;
                                        }

                                        // Exit early if tools keep failing
                                        if consecutive_tool_failures >= 2 {
                                            eprintln!("[syn-abort] consecutive tool failures in JSON calls, aborting synthesis");
                                            final_text = "Error: The requested actions could not be completed (repeated tool failures).".to_string();
                                            break;
                                        }

                                        continue; // next synthesis round
                                    }
                                }

                                // Check for XML tool calls
                                let xml_calls = parse_xml_tool_calls(&raw);
                                if !xml_calls.is_empty() {
                                    // FIX: build tool_calls JSON array with synthetic IDs matching what tool_result will use
                                    let tool_calls_json = serde_json::json!(xml_calls
                                        .iter()
                                        .enumerate()
                                        .map(|(i, (tname, targs))| {
                                            serde_json::json!({
                                                "id": format!("syn_xml_{}", i),
                                                "type": "function",
                                                "function": {
                                                    "name": tname,
                                                    "arguments": targs.to_string()
                                                }
                                            })
                                        })
                                        .collect::<Vec<_>>());
                                    history.messages.push(
                                        crate::llm::ChatMessage::assistant_with_tool_calls(
                                            raw.clone(),
                                            tool_calls_json,
                                        ),
                                    );
                                    let mut xml_tool_results = Vec::new();
                                    for (i, (tname, targs)) in xml_calls.iter().enumerate() {
                                        let cid = format!("syn_xml_{}", i);
                                        eprintln!("[syn-xml-tool] {} args={}", tname, targs);
                                        let result = if let Some(tool) =
                                            self.tool_registry.get(tname)
                                        {
                                            match tool
                                                .call(
                                                    targs.clone(),
                                                    ToolContext {
                                                        audit_log: Arc::new(std::sync::Mutex::new(
                                                            self.audit_log.clone(),
                                                        )),
                                                        filesystem_policy: ctx
                                                            .filesystem_policy
                                                            .clone(),
                                                        network_policy: ctx.network_policy.clone(),
                                                        allowed_tools: ctx.allowed_tools.clone(),
                                                    },
                                                )
                                                .await
                                            {
                                                Ok(o) => {
                                                    eprintln!(
                                                        "[syn-xml-ok] {}: {}",
                                                        tname,
                                                        &o.raw[..o.raw.len().min(80)]
                                                    );
                                                    o.raw
                                                }
                                                Err(e) => {
                                                    eprintln!("[syn-xml-err] {}: {}", tname, e);
                                                    format!("Tool error: {}", e)
                                                }
                                            }
                                        } else {
                                            format!("Tool '{}' not found", tname)
                                        };
                                        xml_tool_results.push(result.clone());
                                        history.messages.push(
                                            crate::llm::ChatMessage::tool_result(cid, result),
                                        );
                                    }

                                    // Track consecutive tool failures
                                    let all_failed = xml_tool_results
                                        .iter()
                                        .all(|r| r.starts_with("Tool error:"));
                                    if all_failed && !xml_tool_results.is_empty() {
                                        consecutive_tool_failures += 1;
                                    } else {
                                        consecutive_tool_failures = 0;
                                    }

                                    // Exit early if tools keep failing
                                    if consecutive_tool_failures >= 2 {
                                        eprintln!("[syn-abort] consecutive tool failures in XML calls, aborting synthesis");
                                        final_text = "Error: The requested actions could not be completed (repeated tool failures).".to_string();
                                        break;
                                    }

                                    // FIX 1: After call_agent succeeds, use its result as final output and exit synthesis
                                    // If the only XML tool called is call_agent, treat its result as completion
                                    if xml_calls.len() == 1 && xml_calls[0].0 == "call_agent" {
                                        // call_agent completed a delegation — use the tool result (last message) as final output
                                        if let Some(last_msg) = history.messages.last() {
                                            if matches!(last_msg.role, crate::llm::ChatRole::Tool) {
                                                final_text = last_msg.content.clone();
                                            }
                                        }
                                        eprintln!("[syn-call-agent-done] exiting synthesis after call_agent success");
                                        break;
                                    }

                                    continue; // next synthesis round
                                }

                                // Text-only response — done
                                let cleaned = strip_xml_tool_calls(&raw);
                                final_text = if !cleaned.trim().is_empty() {
                                    cleaned
                                } else if !raw.is_empty() {
                                    raw
                                } else {
                                    let tool_results: Vec<String> = history
                                        .messages
                                        .iter()
                                        .filter(|m| matches!(m.role, crate::llm::ChatRole::Tool))
                                        .map(|m| m.content.clone())
                                        .collect();
                                    if tool_results.is_empty() {
                                        "Task completed.".to_string()
                                    } else {
                                        format!(
                                            "Task completed.\n{}",
                                            tool_results.last().unwrap_or(&String::new())
                                        )
                                    }
                                };
                                break;
                            }
                            Err(_) => {
                                let tool_results: Vec<String> = history
                                    .messages
                                    .iter()
                                    .filter(|m| matches!(m.role, crate::llm::ChatRole::Tool))
                                    .map(|m| m.content.clone())
                                    .collect();
                                final_text = if tool_results.is_empty() {
                                    "Task completed.".to_string()
                                } else {
                                    format!(
                                        "Task completed.\n{}",
                                        tool_results.last().unwrap_or(&String::new())
                                    )
                                };
                                break;
                            }
                        }
                    }
                }

                Ok(StepOutput::new(final_text))
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
                match super::parallel::execute_parallel_batch(self, pipeline, &mut ctx, &batch).await {
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

            // Compute effective tool scope for this step
            ctx.allowed_tools = crate::toolset::ToolSet::Intersection(
                Box::new(agent.policy.allowed_tools.clone()),
                Box::new(step.tools.clone()),
            );

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
                    eprintln!("[verdict] warning: ContextStore::save failed: {e}");
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_shell_run_command() {
        let args = serde_json::json!({
            "command": "rm",
            "args": ["-rf", "/tmp"]
        });
        let result = extract_shell_command_string("shell.run", &args);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "rm -rf /tmp");
    }

    #[test]
    fn test_extract_shell_run_command_tool_run_command_variant() {
        // Critical test: shell.run_command must extract the command the same way as shell.run
        let args = serde_json::json!({
            "command": "rm",
            "args": ["-rf", "/tmp"]
        });
        let result = extract_shell_command_string("shell.run_command", &args);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "rm -rf /tmp");
    }

    #[test]
    fn test_extract_shell_cargo_test() {
        let args = serde_json::json!({});
        let result = extract_shell_command_string("shell.cargo_test", &args);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cargo test");
    }

    #[test]
    fn test_extract_shell_unknown_fallback() {
        let args = serde_json::json!({});
        let result = extract_shell_command_string("shell.custom_tool", &args);
        assert!(result.is_ok());
        // Should strip the "shell." prefix
        assert_eq!(result.unwrap(), "custom_tool");
    }

    #[test]
    fn test_extract_shell_run_command_with_no_args() {
        let args = serde_json::json!({
            "command": "cargo"
        });
        let result = extract_shell_command_string("shell.run_command", &args);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "cargo");
    }
}
