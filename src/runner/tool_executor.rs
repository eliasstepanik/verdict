use super::PipelineRunner;
use crate::action::{StepError, StepOutput};
use crate::audit::{AuditEntry, AuditEvent};
use crate::context::StepContext;
use crate::runner::OutputEvent;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use tracing::{trace, warn};

/// Extract the actual shell command string from tool args.
/// For shell.* tools, builds a command string that combines the command + args.
/// For tools like shell.cargo_test, shell.cargo_check, returns just the command name.
pub(crate) fn extract_shell_command_string(tool_name: &str, args: &Value) -> Result<String, String> {
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

        // Step 2.25: Rate-limit check — before any resource consumption
        if let Some(rate_limiter_mutex) = &self.rate_limiter {
            if let Ok(mut rate_limiter) = rate_limiter_mutex.lock() {
                if let Err(budget_err) = rate_limiter.check_rate_limit() {
                    return Err(StepError::ActionFailed {
                        reason: format!("rate limit: {}", budget_err),
                    });
                }
            }
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
        let tool_context = crate::tools::ToolContext {
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

    /// Execute an LLM-requested tool call from inside a `ToolUseLoop`.
    ///
    /// SECURITY: every LLM-driven tool dispatch MUST go through here. It delegates to
    /// [`Self::execute_tool_call`], which enforces the step's `allowed_tools` scope and
    /// records `tools_used` / `commands_executed` so the shell allow/denylist guards
    /// (and the tool-call budget) actually see LLM-initiated calls. Calling
    /// `tool_registry.get(..)` + `tool.call(..)` directly here bypasses all of that.
    ///
    /// Unlike a `ToolCall` step, a failure is not fatal to the loop: the error text is
    /// returned so it can be fed back to the model as a tool result and retried.
    pub(crate) async fn execute_llm_tool_call(
        &self,
        tool_name: &str,
        args: &Value,
        ctx: &mut StepContext,
    ) -> String {
        match self.execute_tool_call(tool_name, args, ctx).await {
            Ok(output) => {
                let pe = output
                    .raw
                    .char_indices()
                    .nth(80)
                    .map(|(i, _)| i)
                    .unwrap_or(output.raw.len());
                trace!(tool_name = %tool_name, output_len = output.raw.len(), preview = %&output.raw[..pe], "tool execution succeeded");
                output.raw
            }
            Err(e) => {
                warn!(tool_name = %tool_name, error = %e, "tool execution failed");
                format!("Tool error: {}", e)
            }
        }
    }

    /// Handle a ToolCall step action, delegating to execute_tool_call and recording audit events.
    pub(crate) async fn handle_tool_call(
        &mut self,
        ctx: &mut StepContext,
        tool: &str,
        args: &Value,
    ) -> Result<StepOutput, StepError> {
        // Emit ToolCallStarted to the real audit log before the call
        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: ctx.pipeline_name.clone(),
            step_name: ctx.step_name.clone(),
            event: AuditEvent::ToolCallStarted {
                tool: tool.to_string(),
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
                        tool: tool.to_string(),
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
                        tool: tool.to_string(),
                        reason: e.to_string(),
                    },
                });
            }
        }

        result
    }
}

#[cfg(test)]
#[path = "tool_executor_tests.rs"]
mod tests;
