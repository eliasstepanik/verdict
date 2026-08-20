// LLM-related functionality: template resolution, XML tool parsing, and LLM call handlers.
// Extracted from execution.rs (Task 2).

use crate::action::{ProviderSpec, StepError, StepOutput};
use crate::context::StepContext;

/// Resolve template placeholders in a string using step context values.
///
/// Substitutes:
/// - `{input}` → ctx.input (or "task" field if object)
/// - `{step_name}` → prior step's output for that step name
///
/// NOTE: O(n²) performance when many placeholders exist — addressed in future optimization phase.
/// If a step output contains literal `{...}` patterns matching step names, those patterns may be
/// substituted in subsequent iterations, potentially causing unexpected cascading replacements.
/// For safety, avoid step outputs that contain literal `{...}` patterns matching step names.
pub fn resolve_template(template: &str, ctx: &StepContext) -> String {
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
pub fn strip_xml_tool_calls(text: &str) -> String {
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
pub fn parse_xml_tool_calls(text: &str) -> Vec<(String, serde_json::Value)> {
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

use crate::runner::PipelineRunner;

impl PipelineRunner {
    /// Handle LlmCall action: call LLM with system/user prompts and conversation history.
    pub(crate) async fn handle_llm_call(
        &self,
        system: &str,
        user: &str,
        model: &Option<ProviderSpec>,
        conversation_id: &Option<String>,
        append_to_history: &bool,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        let llm_client = self
            .llm_client
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
        let response = llm_client
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

    /// Handle LlmCallStreaming action: stream LLM response with chunks emitted to output sink.
    pub(crate) async fn handle_llm_call_streaming(
        &self,
        system: &str,
        user: &str,
        model: &Option<ProviderSpec>,
        conversation_id: &Option<String>,
        append_to_history: &bool,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        use futures::StreamExt;

        let llm_client = self
            .llm_client
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
                sink.emit(crate::runner::OutputEvent::LlmChunk {
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
}
