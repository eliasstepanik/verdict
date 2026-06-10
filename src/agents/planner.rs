//! Planner agent: produces structured execution plans

use crate::action::StepAction;
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guard::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Creates a planner agent that produces structured execution plans.
///
/// The planner agent is conservative: it only has read access to tools,
/// cannot self-update, and has limited delegation depth.
pub fn planner_agent() -> Agent {
    let step = AgentStep {
        name: "plan".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a planning agent. Produce a structured execution plan.".into(),
            user: "Task: {task}\n\nProduce a plan with: steps, affected files, risks, required tools, test strategy.".into(),
            model: None,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let pipeline = Pipeline {
        name: "planner_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 3,
    };

    let policy = AgentPolicy {
        max_steps: 20,
        max_retries: 3,
        max_delegation_depth: 1,
        max_cost_usd: Some(5.0),
        max_runtime_seconds: Some(300),
        allow_self_update: false,
        require_approval_for_self_update: true,
        allowed_agents: vec![],
        allowed_tools: ToolSet::ReadOnly,
        allowed_skills: vec!["api_design".to_string()],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy::default(),
    };

    Agent {
        name: "planner".into(),
        description: "Produces structured execution plans.".into(),
        pipeline,
        tools: ToolSet::ReadOnly,
        skills: SkillSet::with_skills(vec!["api_design".to_string()]),
        policy,
    }
}
