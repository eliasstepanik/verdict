// LLM call handlers: async handlers for LlmCall and LlmCallStreaming actions.
// Extracted from execution.rs (Task 2).
// Template resolution and XML parsing moved to xml_tools.rs.

use crate::action::{ProviderSpec, StepError, StepOutput};
use crate::context::StepContext;
use crate::runner::xml_tools::resolve_template;
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

        // Call the LLM (rate limiting is now handled inside LlmClient::complete())
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

        // Stream the response (rate limiting is now handled inside LlmClient::stream())
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
