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

    /// Record an LLM call cost and decrement remaining budget
    pub(crate) fn record_llm_cost(&self, usage: &crate::llm::LlmUsage, ctx: &mut StepContext) {
        // Stub: rough estimation based on token counts (gpt-4o pricing example)
        let prompt_cost = (usage.prompt_tokens as f64) * 0.00003; // $0.03 per 1M
        let completion_cost = (usage.completion_tokens as f64) * 0.00006; // $0.06 per 1M
        let total_cost = prompt_cost + completion_cost;

        if let Some(ref mut remaining) = ctx.budget.remaining_usd {
            *remaining -= total_cost;
        }
    }

    /// Record a tool call cost and increment tool_calls_used
    pub(crate) fn record_tool_call_cost(&self, _ctx: &mut StepContext) {
        // Stub: tool calls typically have low/no cost
        // Could be extended for paid APIs
    }
}
