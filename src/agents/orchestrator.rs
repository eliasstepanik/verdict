//! Orchestrator agent: delegates work to specialized agents

use crate::action::StepAction;
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guard::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Creates an orchestrator agent that delegates work to specialized agents.
///
/// The orchestrator agent is read-only and coordinates the work of other agents
/// (planner, coder, reviewer, debugger, reflector) to achieve user goals.
pub fn orchestrator_agent() -> Agent {
    let step = AgentStep {
        name: "orchestrate".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are an orchestrator agent. Coordinate specialized agents to achieve the user's goal.".into(),
            user: "Goal: {goal}\n\nAvailable agents: planner, coder, reviewer, debugger, reflector.\n\nProduce a delegation plan.".into(),
            model: None,
            conversation_id: None,
            append_to_history: true,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: Vec::new(),
        parallel: false,
    };

    let pipeline = Pipeline {
        name: "orchestrator_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 3,
    };

    let policy = AgentPolicy {
        max_steps: 20,
        max_retries: 3,
        max_delegation_depth: 3,
        max_cost_usd: Some(5.0),
        max_runtime_seconds: Some(300),
        allow_self_update: false,
        require_approval_for_self_update: true,
        allowed_agents: vec![
            "planner".to_string(),
            "coder".to_string(),
            "reviewer".to_string(),
            "debugger".to_string(),
            "reflector".to_string(),
        ],
        allowed_tools: ToolSet::ReadOnly,
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy::default(),
    };

    Agent {
        name: "orchestrator".into(),
        description: "Delegates work to specialized agents to achieve user goals.".into(),
        pipeline,
        tools: ToolSet::ReadOnly,
        skills: SkillSet::default(),
        policy,
    }
}
