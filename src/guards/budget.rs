use super::GuardError;
use crate::context::StepContext;

pub fn check_max_cost_usd(
    _guard: &super::Guard,
    ctx: &StepContext,
    max_usd: f64,
) -> Result<(), GuardError> {
    if let Some(remaining) = ctx.budget.remaining_usd {
        if remaining < 0.0 {
            return Err(GuardError::Failed {
                guard: "MaxCostUsd".into(),
                reason: format!("Budget exhausted: remaining={:.4}", remaining),
            });
        }
        if remaining < -max_usd {
            return Err(GuardError::Failed {
                guard: "MaxCostUsd".into(),
                reason: format!("Cost exceeded ${:.4} cap", max_usd),
            });
        }
    }
    Ok(())
}

pub fn check_max_llm_calls(
    _guard: &super::Guard,
    ctx: &StepContext,
    max_calls: u32,
) -> Result<(), GuardError> {
    if ctx.budget.llm_calls_used <= max_calls {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "MaxLlmCalls".to_string(),
            reason: format!(
                "LLM calls exceeded: {}/{}",
                ctx.budget.llm_calls_used, max_calls
            ),
        })
    }
}

pub fn check_max_tool_calls(
    _guard: &super::Guard,
    ctx: &StepContext,
    max_calls: u32,
) -> Result<(), GuardError> {
    if ctx.budget.tool_calls_used <= max_calls {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "MaxToolCalls".to_string(),
            reason: format!(
                "tool calls exceeded: {}/{}",
                ctx.budget.tool_calls_used, max_calls
            ),
        })
    }
}

pub fn check_max_delegation_depth(
    _guard: &super::Guard,
    ctx: &StepContext,
    max_depth: u32,
) -> Result<(), GuardError> {
    if ctx.delegation_depth <= max_depth {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "MaxDelegationDepth".to_string(),
            reason: format!(
                "delegation depth exceeded: {}/{}",
                ctx.delegation_depth, max_depth
            ),
        })
    }
}

pub fn check_timeout_seconds(
    _guard: &super::Guard,
    ctx: &StepContext,
    max_secs: u64,
) -> Result<(), GuardError> {
    let elapsed = ctx.budget.start_time.elapsed().as_secs();
    if elapsed < max_secs {
        Ok(())
    } else {
        Err(GuardError::Failed {
            guard: "TimeoutSeconds".to_string(),
            reason: format!("timeout exceeded: {}s/{}s", elapsed, max_secs),
        })
    }
}
