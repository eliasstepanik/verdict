use super::PipelineError;
use super::PipelineRunner;
use crate::action::StepError;
use crate::agent::Agent;
use crate::context::StepContext;

#[allow(dead_code)]
impl PipelineRunner {
    /// Check agent policy limits before proceeding with next step
    pub(crate) fn check_policy_limits(
        &self,
        step_count: usize,
        agent: &Agent,
        ctx: &StepContext,
    ) -> Result<(), PipelineError> {
        // Check max steps
        if step_count as u32 > agent.policy.max_steps {
            return Err(PipelineError::StepFailed {
                step: ctx.step_name.clone(),
                error: StepError::ActionFailed {
                    reason: format!("Max steps {} exceeded", agent.policy.max_steps),
                },
            });
        }

        // Check max cost (if set)
        if let Some(max_cost) = agent.policy.max_cost_usd {
            if let Some(remaining) = ctx.budget.remaining_usd {
                if remaining <= 0.0 {
                    return Err(PipelineError::StepFailed {
                        step: ctx.step_name.clone(),
                        error: StepError::ActionFailed {
                            reason: format!("Cost budget exhausted (max: ${:.2})", max_cost),
                        },
                    });
                }
            }
        }

        // Check max runtime (if set)
        if let Some(max_secs) = agent.policy.max_runtime_seconds {
            let elapsed = ctx.budget.start_time.elapsed().as_secs();
            if elapsed > max_secs {
                return Err(PipelineError::StepFailed {
                    step: ctx.step_name.clone(),
                    error: StepError::ActionFailed {
                        reason: format!("Runtime limit {} seconds exceeded", max_secs),
                    },
                });
            }
        }

        Ok(())
    }

    /// Record an LLM call cost based on model and token usage
    /// Pricing: input tokens * input_rate + output tokens * output_rate
    /// Rates in $/token: $0.03 per 1M tokens = $0.00000003 per token
    pub(crate) fn record_llm_cost(&self, usage: &crate::llm::LlmUsage, model: &str, ctx: &mut StepContext) {
        // Pricing rates per token for common models
        // Format: (input_rate_per_token, output_rate_per_token)
        let (input_rate, output_rate) = match model {
            // GPT-4 Turbo: $0.01/$0.03 per 1K tokens = $0.00001/$0.00003 per token
            m if m.contains("gpt-4-turbo") || m.contains("gpt-4-1106") => (0.00001, 0.00003),
            // GPT-4o: $0.005/$0.015 per 1K tokens = $0.000005/$0.000015 per token
            m if m.contains("gpt-4o") => (0.000005, 0.000015),
            // GPT-3.5 Turbo: $0.0005/$0.0015 per 1K tokens = $0.0000005/$0.0000015 per token
            m if m.contains("gpt-3.5") => (0.0000005, 0.0000015),
            // Claude 3 Opus: $0.015/$0.075 per 1K tokens
            m if m.contains("claude-3-opus") => (0.000015, 0.000075),
            // Claude 3 Sonnet: $0.003/$0.015 per 1K tokens
            m if m.contains("claude-3-sonnet") => (0.000003, 0.000015),
            // Claude 3 Haiku: $0.00025/$0.00125 per 1K tokens
            m if m.contains("claude-3-haiku") => (0.00000025, 0.00000125),
            // Default fallback: GPT-4o rates
            _ => (0.000005, 0.000015),
        };

        let prompt_cost = (usage.prompt_tokens as f64) * input_rate;
        let completion_cost = (usage.completion_tokens as f64) * output_rate;
        let total_cost = prompt_cost + completion_cost;

        ctx.budget.spent_usd += total_cost;
        if let Some(ref mut remaining) = ctx.budget.remaining_usd {
            *remaining -= total_cost;
        }
    }

    /// Record a tool call and increment tool_calls_used counter
    pub(crate) fn record_tool_call_cost(&self, ctx: &mut StepContext) {
        // Tool calls typically have no direct cost, but we track usage for rate limiting
        ctx.budget.tool_calls_used += 1;
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn test_record_llm_cost_gpt4o() {
        // GPT-4o: $0.005/$0.015 per 1K = $0.000005/$0.000015 per token
        let runner = PipelineRunner::new();
        let mut ctx = crate::context::StepContext::new(
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            serde_json::json!({}),
            crate::agent::FilesystemPolicy::default(),
        );
        ctx.budget.remaining_usd = Some(100.0);

        let usage = crate::llm::LlmUsage {
            prompt_tokens: 1000,
            completion_tokens: 500,
        };

        runner.record_llm_cost(&usage, "gpt-4o", &mut ctx);

        // Expected: 1000 * 0.000005 + 500 * 0.000015 = 0.005 + 0.0075 = 0.0125
        assert!((ctx.budget.spent_usd - 0.0125).abs() < 0.000001);
        assert!((ctx.budget.remaining_usd.unwrap() - 99.9875).abs() < 0.000001);
    }

    #[test]
    fn test_record_llm_cost_gpt35() {
        // GPT-3.5 Turbo: $0.0005/$0.0015 per 1K = $0.0000005/$0.0000015 per token
        let runner = PipelineRunner::new();
        let mut ctx = crate::context::StepContext::new(
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            serde_json::json!({}),
            crate::agent::FilesystemPolicy::default(),
        );
        ctx.budget.remaining_usd = Some(100.0);

        let usage = crate::llm::LlmUsage {
            prompt_tokens: 1000,
            completion_tokens: 500,
        };

        runner.record_llm_cost(&usage, "gpt-3.5-turbo", &mut ctx);

        // Expected: 1000 * 0.0000005 + 500 * 0.0000015 = 0.0005 + 0.00075 = 0.00125
        assert!((ctx.budget.spent_usd - 0.00125).abs() < 0.000001);
    }

    #[test]
    fn test_record_llm_cost_catches_1000x_error() {
        // This test verifies the 1000x error is FIXED.
        // Old buggy code: *0.00003 would compute 1000x higher cost
        // New code: *0.000005 (GPT-4o) is correct
        let runner = PipelineRunner::new();
        let mut ctx = crate::context::StepContext::new(
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            serde_json::json!({}),
            crate::agent::FilesystemPolicy::default(),
        );
        ctx.budget.remaining_usd = Some(100.0);

        let usage = crate::llm::LlmUsage {
            prompt_tokens: 1_000_000, // 1M tokens
            completion_tokens: 500_000, // 500K tokens
        };

        runner.record_llm_cost(&usage, "gpt-4o", &mut ctx);

        // GPT-4o: $0.005 per 1K = $0.000005 per token
        // Cost: 1M * 0.000005 + 500K * 0.000015 = 5 + 7.5 = $12.50
        let expected_cost = 5.0 + 7.5;
        assert!((ctx.budget.spent_usd - expected_cost).abs() < 0.001);

        // If the old bug were present (*0.00003), it would be 1000x higher ≈ $12500
        // Verify we're NOT at that value
        assert!(ctx.budget.spent_usd < 100.0, "Cost should not be thousands of dollars");
    }

    #[test]
    fn test_record_tool_call_increments_counter() {
        let runner = PipelineRunner::new();
        let mut ctx = crate::context::StepContext::new(
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            serde_json::json!({}),
            crate::agent::FilesystemPolicy::default(),
        );

        assert_eq!(ctx.budget.tool_calls_used, 0);

        runner.record_tool_call_cost(&mut ctx);
        assert_eq!(ctx.budget.tool_calls_used, 1);

        runner.record_tool_call_cost(&mut ctx);
        assert_eq!(ctx.budget.tool_calls_used, 2);
    }

    #[test]
    fn test_record_tool_call_increments_without_cost() {
        // Tool calls increment counter but don't add cost
        let runner = PipelineRunner::new();
        let mut ctx = crate::context::StepContext::new(
            "test".to_string(),
            "test".to_string(),
            "test".to_string(),
            serde_json::json!({}),
            crate::agent::FilesystemPolicy::default(),
        );
        ctx.budget.remaining_usd = Some(100.0);

        runner.record_tool_call_cost(&mut ctx);

        assert_eq!(ctx.budget.tool_calls_used, 1);
        assert_eq!(ctx.budget.spent_usd, 0.0);
        assert_eq!(ctx.budget.remaining_usd, Some(100.0));
    }
}
