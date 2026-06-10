//! Reflector agent: analyzes agent performance and suggests improvements

use crate::action::StepAction;
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guard::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Creates a reflector agent that analyzes agent performance and suggests improvements.
///
/// The reflector agent is read-only and analyzes pipeline traces to identify
/// patterns and opportunities for improvement.
pub fn reflector_agent() -> Agent {
    let step = AgentStep {
        name: "reflect".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a reflection agent. Analyze the pipeline trace and suggest improvements.".into(),
            user: "Trace: {trace}\n\nFailures: {failures}\n\nTool calls: {tool_calls}\n\nOutput: what_worked, what_failed, suggested_improvement, proposed_patch_category, risk_level.".into(),
            model: None,
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
        name: "reflector_pipeline".into(),
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
        allowed_skills: vec![],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy::default(),
    };

    Agent {
        name: "reflector".into(),
        description: "Analyzes agent performance and suggests improvements.".into(),
        pipeline,
        tools: ToolSet::ReadOnly,
        skills: SkillSet::default(),
        policy,
    }
}
