// Synthesis loop implementation: XML-based tool-call retry when main loop exits.
// Extracted from tool_use_loop.rs (Task 2, phase 2).

use crate::action::{ProviderSpec, StepError};
use crate::context::StepContext;
use crate::runner::PipelineRunner;

impl PipelineRunner {
    /// Run synthesis loop: retry with XML tool calls to complete the task.
    pub(crate) async fn run_synthesis_loop(
        &self,
        llm_client: &std::sync::Arc<crate::llm::LlmClient>,
        resolved_system: &str,
        model: &ProviderSpec,
        tool_schemas: &[crate::llm::ToolSchema],
        tool_name_map: &std::collections::HashMap<String, String>,
        history: &mut crate::llm::MessageHistory,
        ctx: &mut StepContext,
    ) -> Result<String, StepError> {
        use tracing::{debug, warn};
        use super::xml_tools::{parse_xml_tool_calls, strip_xml_tool_calls};

        let tool_list = tool_schemas
            .iter()
            .map(|t| format!("  - {} : {}", t.name, t.description))
            .collect::<Vec<_>>()
            .join("\n");

        let xml_instruction = format!(
            "Please complete the task now. Available tools:\n{}\n\nCall ONE tool at a time using XML:\n<invoke name=\"TOOL_NAME\"><parameter name=\"KEY\">VALUE</parameter></invoke>\n\nWait for each tool result before calling the next tool, especially when you need the ID from one call to use in the next.\n\nAfter all tool calls, provide your final text answer.",
            tool_list
        );

        history.push(crate::llm::ChatRole::User, xml_instruction);

        let mut final_text = String::new();
        let mut consecutive_tool_failures = 0;

        for _syn_round in 0..10 {
            let synthesis_req = crate::llm::LlmRequest {
                system: resolved_system.to_string(),
                user: String::new(),
                model: if model.model.is_empty() {
                    llm_client.default_model().to_string()
                } else {
                    model.model.clone()
                },
                max_tokens: None,
                history: Some(history.clone()),
                temperature: None,
                // No API tools — model uses XML <invoke> format
                tools: None,
                tool_choice: None,
            };

            // Rate-limit check before synthesis LLM call — MUST be before match to surface errors
            if let Some(rate_limiter_mutex) = &self.rate_limiter {
                if let Ok(mut rate_limiter) = rate_limiter_mutex.lock() {
                    if let Err(budget_err) = rate_limiter.check_rate_limit() {
                        return Err(StepError::ActionFailed {
                            reason: format!("rate limit: {}", budget_err),
                        });
                    }
                }
            }

            match llm_client.complete(synthesis_req).await {
                Ok(syn_resp) => {
                    ctx.budget.llm_calls_used += 1;

                    if let Some(usage) = &syn_resp.usage {
                        self.record_llm_cost(usage, &syn_resp.model, ctx);
                    }

                    let raw = syn_resp.content.clone();

                    // Handle JSON tool calls in synthesis response
                    if let Some(ref tcs) = syn_resp.tool_calls {
                        if !tcs.is_empty() {
                            self.handle_synthesis_json_tools(
                                tcs,
                                tool_name_map,
                                &raw,
                                history,
                                ctx,
                                &mut consecutive_tool_failures,
                                &mut final_text,
                            )
                            .await?;
                            continue;
                        }
                    }

                    // Check for XML tool calls
                    let xml_calls = parse_xml_tool_calls(&raw);
                    if !xml_calls.is_empty() {
                        // Build tool_calls JSON with synthetic IDs
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
                            let result = self.execute_llm_tool_call(tname, targs, ctx).await;
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

                        if consecutive_tool_failures >= 2 {
                            warn!("consecutive tool failures in XML calls, aborting synthesis");
                            final_text =
                                "Error: The requested actions could not be completed (repeated tool failures)."
                                    .to_string();
                            break;
                        }

                        // If call_agent was the only tool, exit
                        if xml_calls.len() == 1 && xml_calls[0].0 == "call_agent" {
                            if let Some(last_msg) = history.messages.last() {
                                if matches!(last_msg.role, crate::llm::ChatRole::Tool) {
                                    final_text = last_msg.content.clone();
                                }
                            }
                            debug!("exiting synthesis after call_agent success");
                            break;
                        }

                        continue;
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
                Err(llm_err) => {
                    return Err(StepError::ActionFailed {
                        reason: format!("synthesis LLM call failed: {}", llm_err),
                    });
                }
            }
        }

        Ok(final_text)
    }

    /// Handle JSON tool calls within synthesis loop.
    pub(crate) async fn handle_synthesis_json_tools(
        &self,
        tcs: &[crate::llm::ToolCall],
        tool_name_map: &std::collections::HashMap<String, String>,
        raw: &str,
        history: &mut crate::llm::MessageHistory,
        ctx: &mut StepContext,
        consecutive_tool_failures: &mut u32,
        final_text: &mut String,
    ) -> Result<(), StepError> {
        use tracing::warn;

        let tc_json = serde_json::json!(tcs
            .iter()
            .enumerate()
            .map(|(i, tc)| {
                let cid = tc.id.clone().unwrap_or_else(|| format!("syn_call_{}", i));
                serde_json::json!({
                    "id": cid,
                    "type": "function",
                    "function": {
                        "name": tc.name,
                        "arguments": tc.arguments.to_string()
                    }
                })
            })
            .collect::<Vec<_>>());
        history.messages.push(
            crate::llm::ChatMessage::assistant_with_tool_calls(raw.to_string(), tc_json),
        );

        let mut tool_results = Vec::new();
        for (i, tc) in tcs.iter().enumerate() {
            let cid = tc.id.clone().unwrap_or_else(|| format!("syn_call_{}", i));
            let rname = tool_name_map
                .get(&tc.name)
                .cloned()
                .unwrap_or_else(|| tc.name.clone());
            let result = self.execute_llm_tool_call(&rname, &tc.arguments, ctx).await;
            tool_results.push(result.clone());
            history
                .messages
                .push(crate::llm::ChatMessage::tool_result(cid, result));
        }

        // Track consecutive tool failures
        let all_failed = tool_results.iter().all(|r| r.starts_with("Tool error:"));
        if all_failed && !tool_results.is_empty() {
            *consecutive_tool_failures += 1;
        } else {
            *consecutive_tool_failures = 0;
        }

        if *consecutive_tool_failures >= 2 {
            warn!("consecutive tool failures in JSON calls, aborting synthesis");
            *final_text =
                "Error: The requested actions could not be completed (repeated tool failures)."
                    .to_string();
        }

        Ok(())
    }
}
