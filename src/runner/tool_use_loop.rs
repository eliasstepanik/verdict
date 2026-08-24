// Tool-use loop implementation: handles multi-round LLM + tool-call interaction.
// Extracted from execution.rs (Task 2).
//
// ponytail: this file exceeds the 300-line limit as a documented, user-approved exception.
// Rationale: handle_tool_use_loop's round-loop and handle_tool_calls_in_loop's JSON/XML
// dual-path handling form a tightly-coupled state machine (shared MessageHistory mutation,
// round-counter state, stop-condition evaluation across formats). The JSON/XML branch
// duplication in handle_tool_calls_in_loop is a DELIBERATE safeguard against divergent-path
// bugs (this exact bug class was found and fixed 8 times in an earlier session — see
// notes/verdict-audit-cycle1-fix-plan.md and the H1/C2/C3/C4 fixes). Splitting these
// functions further risks reintroducing that bug class. Approved as an escalation-clause
// exception per AGENTS.md 300-Line File Limit rule (tightly-coupled state machine).

use crate::action::{ProviderSpec, StepError, StepOutput};
use crate::context::StepContext;
use crate::injection::sanitize_for_exposure;
use crate::pipeline::InjectionProtection;
use crate::runner::PipelineRunner;

/// Shared gate for tool-result sanitization: applies injection protection scanning
/// if InjectionProtection::Strict is configured, otherwise returns result unchanged.
/// This function is the SINGLE enforcement point for all 4 tool-result codepaths
/// (JSON main-loop, XML main-loop, JSON synthesis, XML synthesis) to prevent
/// divergent-gate bugs.
pub(crate) fn gate_tool_result(ctx: &StepContext, result: String) -> String {
    if ctx.injection_protection == InjectionProtection::Strict {
        sanitize_for_exposure(&result)
    } else {
        result
    }
}

impl PipelineRunner {
    /// Handle ToolUseLoop action: multi-round LLM conversation with tool execution.
    ///
    /// Main flow:
    /// 1. For each round (up to max_rounds), call LLM with tool schemas.
    /// 2. If LLM returns tool calls (JSON or XML), execute them and append results to history.
    /// 3. If LLM returns text-only response and stop_condition is met, exit.
    /// 4. After main loop, if synthesis is needed, run a synthesis loop that accepts XML tool calls.
    ///
    /// The loop preserves conversation history across all rounds, enabling context-aware
    /// multi-turn interactions.
    pub(crate) async fn handle_tool_use_loop(
        &self,
        system: &str,
        user: &str,
        model: &ProviderSpec,
        tools: &[String],
        max_rounds: &usize,
        stop_condition: &crate::action::StopCondition,
        ctx: &mut StepContext,
    ) -> Result<StepOutput, StepError> {
        use tracing::trace;
        use super::xml_tools::{resolve_template, parse_xml_tool_calls};

        let llm_client = self
            .llm_client
            .as_ref()
            .ok_or_else(|| StepError::ActionFailed {
                reason: "LLM client not configured".into(),
            })?;

        // Build tool schemas and sanitize tool names (dots → underscores for LLM).
        let (tool_schemas, tool_name_map) = self.build_tool_schemas(tools);

        let resolved_system = resolve_template(system, ctx);
        let resolved_user = resolve_template(user, ctx);

        trace!(
            system_len = resolved_system.len(),
            user_len = resolved_user.len(),
            user_preview = %&resolved_user[..resolved_user.len().min(80)],
            "tool loop initialized"
        );

        // Conversation history accumulated across rounds.
        let mut history = crate::llm::MessageHistory::new();
        let mut final_text = String::new();
        let mut answered_with_text = false;

        // Main loop: execute tool calls until stop condition met.
        for round in 0..*max_rounds {
            let user_msg = if round == 0 {
                resolved_user.clone()
            } else {
                // History already contains tool results; no user message needed.
                String::new()
            };

            let (response, mut should_stop) = self
                .run_tool_loop_round(
                    &llm_client,
                    &resolved_system,
                    &resolved_user,
                    model,
                    &tool_schemas,
                    &tool_name_map,
                    &history,
                    round,
                    max_rounds,
                    stop_condition,
                    ctx,
                )
                .await?;

            final_text = response.content.clone();
            let has_tool_calls = response
                .tool_calls
                .as_ref()
                .map_or(false, |tc| !tc.is_empty());
            let xml_tool_calls = if !has_tool_calls {
                parse_xml_tool_calls(&final_text)
            } else {
                Vec::new()
            };

            // For TextOnly, check only after XML parsing
            if !should_stop && matches!(stop_condition, crate::action::StopCondition::TextOnly) {
                should_stop = !has_tool_calls && xml_tool_calls.is_empty();
            }

            if should_stop {
                // Add this exchange to history before stopping (for context)
                if !user_msg.is_empty() {
                    history.push(crate::llm::ChatRole::User, user_msg.clone());
                }
                history.push(crate::llm::ChatRole::Assistant, final_text.clone());
                // The assistant answered in prose with nothing left to execute:
                // this text IS the result, so no synthesis pass is needed.
                answered_with_text =
                    !has_tool_calls && xml_tool_calls.is_empty() && !final_text.trim().is_empty();
                break;
            }

            // Add user message to history (round 0 only — subsequent rounds have tool msgs)
            if !user_msg.is_empty() {
                history.push(crate::llm::ChatRole::User, user_msg.clone());
            }

            // Execute tool calls and add to history
            self.handle_tool_calls_in_loop(
                &llm_client,
                &response,
                &xml_tool_calls,
                &tool_name_map,
                &mut history,
                ctx,
            )
            .await?;
        }

        // Synthesis: if needed, run an XML-based tool-call pass to complete the task.
        if !history.is_empty() && !tool_schemas.is_empty() && !answered_with_text {
            final_text = self
                .run_synthesis_loop(
                    &llm_client,
                    &resolved_system,
                    model,
                    &tool_schemas,
                    &tool_name_map,
                    &mut history,
                    ctx,
                )
                .await?;
        }

        Ok(StepOutput::new(final_text))
    }

    /// Execute a single round of the tool-use loop: call LLM, check stop condition.
    /// Returns (response, should_stop).
    /// Note: should_stop only considers Pattern and MaxRounds; TextOnly is checked by the caller
    /// after XML parsing.
    async fn run_tool_loop_round(
        &self,
        llm_client: &std::sync::Arc<crate::llm::LlmClient>,
        resolved_system: &str,
        resolved_user: &str,
        model: &ProviderSpec,
        tool_schemas: &[crate::llm::ToolSchema],
        _tool_name_map: &std::collections::HashMap<String, String>,
        history: &crate::llm::MessageHistory,
        round: usize,
        max_rounds: &usize,
        stop_condition: &crate::action::StopCondition,
        ctx: &mut StepContext,
    ) -> Result<(crate::llm::LlmResponse, bool), StepError> {
        use tracing::trace;

        let user_msg = if round == 0 {
            resolved_user.to_string()
        } else {
            String::new()
        };

        let tool_choice = if !tool_schemas.is_empty() {
            Some("auto".to_string())
        } else {
            None
        };

        // Belt and suspenders: append instruction to system prompt on round 0
        let effective_system = if round == 0 && !tool_schemas.is_empty() {
            format!("{}\n\nWICHTIG: Du MUSST jetzt ein Tool aufrufen. Antworte NICHT mit Text — rufe sofort das passende Tool auf.", resolved_system)
        } else {
            resolved_system.to_string()
        };

        let req = crate::llm::LlmRequest {
            system: effective_system,
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
                Some(tool_schemas.to_vec())
            },
            tool_choice,
        };

        // Rate limiting is now handled inside LlmClient::complete()
        let response = llm_client
            .complete(req)
            .await
            .map_err(|e| StepError::ActionFailed {
                reason: format!("ToolUseLoop LLM call failed: {}", e),
            })?;

        ctx.budget.llm_calls_used += 1;

        if let Some(usage) = &response.usage {
            self.record_llm_cost(usage, &response.model, ctx);
        }

        // Check stop condition (Pattern and MaxRounds only; TextOnly handled by caller)
        let should_stop = match stop_condition {
            crate::action::StopCondition::Pattern(pattern) => response.content.contains(pattern),
            crate::action::StopCondition::MaxRounds => round + 1 >= *max_rounds,
            crate::action::StopCondition::TextOnly => false, // Caller will check this after XML parse
        };

        trace!(
            round = round,
            has_tool_calls = response.tool_calls.as_ref().map_or(false, |tc| !tc.is_empty()),
            should_stop = should_stop,
            "tool loop round completed"
        );

        Ok((response, should_stop))
    }

    /// Execute tool calls (both JSON and XML) and add results to history.
    async fn handle_tool_calls_in_loop(
        &self,
        _llm_client: &std::sync::Arc<crate::llm::LlmClient>,
        response: &crate::llm::LlmResponse,
        xml_tool_calls: &[(String, serde_json::Value)],
        tool_name_map: &std::collections::HashMap<String, String>,
        history: &mut crate::llm::MessageHistory,
        ctx: &mut StepContext,
    ) -> Result<(), StepError> {
        use tracing::trace;

        if let Some(ref tool_calls_list) = response.tool_calls {
            // Build the tool_calls array in OpenAI format
            let tool_calls_json = serde_json::json!(tool_calls_list
                .iter()
                .enumerate()
                .map(|(i, tc)| {
                    let call_id = tc.id.clone().unwrap_or_else(|| format!("call_{}", i));
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
            history.messages.push(
                crate::llm::ChatMessage::assistant_with_tool_calls(
                    response.content.clone(),
                    tool_calls_json,
                ),
            );

            // Execute each tool and add result
            for (i, tc) in tool_calls_list.iter().enumerate() {
                let call_id = tc.id.clone().unwrap_or_else(|| format!("call_{}", i));
                let registry_name = tool_name_map
                    .get(&tc.name)
                    .cloned()
                    .unwrap_or_else(|| tc.name.clone());

                trace!(
                    llm_name = %tc.name,
                    registry_name = %registry_name,
                    args = %tc.arguments,
                    "LLM tool call"
                );

                let tool_result = self
                    .execute_llm_tool_call(&registry_name, &tc.arguments, ctx)
                    .await;

                // Gate: apply intermediate-response scanning via shared enforcement point
                let sanitized_result = gate_tool_result(ctx, tool_result);

                history
                    .messages
                    .push(crate::llm::ChatMessage::tool_result(call_id, sanitized_result));
            }
        } else if !xml_tool_calls.is_empty() {
            // FIX: build tool_calls JSON array with synthetic IDs
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
            history.messages.push(
                crate::llm::ChatMessage::assistant_with_tool_calls(
                    response.content.clone(),
                    tool_calls_json,
                ),
            );

            // Execute each XML tool call
            for (i, (tool_name, args)) in xml_tool_calls.iter().enumerate() {
                let call_id = format!("xml_call_{}", i);
                trace!(tool_name = %tool_name, args = %args, "XML tool call");

                let tool_result = self.execute_llm_tool_call(tool_name, args, ctx).await;
                
                // Gate: apply intermediate-response scanning via shared enforcement point
                let sanitized_result = gate_tool_result(ctx, tool_result);

                history.messages.push(
                    crate::llm::ChatMessage::tool_result(call_id, sanitized_result),
                );
            }
        } else {
            // No tool calls — plain assistant message
            history.push(
                crate::llm::ChatRole::Assistant,
                response.content.clone(),
            );
        }

        Ok(())
    }
}
