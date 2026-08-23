//! Shared builders and utilities for delegation test probes.

use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use verdict::prelude::*;

/// A tool that records the fact it was actually executed.
pub fn tripwire_tool(flag: Arc<AtomicBool>) -> FunctionTool {
    FunctionTool::new(
        "test.denied_tool",
        "Flips a shared flag when executed.",
        json!({ "type": "object", "properties": {} }),
        move |_args, _ctx| {
            let flag = flag.clone();
            Box::pin(async move {
                flag.store(true, Ordering::SeqCst);
                Ok(ToolOutput::text("tool ran".into()))
            })
        },
    )
}

pub fn step(name: &str, action: StepAction, tools: ToolSet) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action,
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

pub fn pipeline(name: &str, steps: Vec<AgentStep>) -> Pipeline {
    Pipeline {
        name: name.into(),
        steps,
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

pub fn agent(name: &str, p: Pipeline, allowed_tools: ToolSet) -> Agent {
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = allowed_tools.clone();
    policy.allowed_agents = vec!["level2".into()];
    Agent {
        name: name.into(),
        description: name.into(),
        pipeline: p,
        tools: allowed_tools,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    }
}
