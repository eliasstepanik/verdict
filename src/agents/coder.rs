//! Coder agent: implements approved software changes

use crate::action::StepAction;
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guard::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Creates a coder agent that implements approved software changes.
///
/// The coder agent has read-write access to tools, allowing it to modify
/// files and run tests. It can delegate to other agents up to 2 levels deep.
pub fn coder_agent() -> Agent {
    let step = AgentStep {
        name: "implement".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a coding agent. Implement the requested changes.".into(),
            user: "Implement the requested changes. Produce a diff.".into(),
            model: None,
            conversation_id: None,
            append_to_history: true,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadWrite,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: Vec::new(),
        parallel: false,
    };

    let pipeline = Pipeline {
        name: "coder_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 3,
    };

    let policy = AgentPolicy {
        max_steps: 20,
        max_retries: 3,
        max_delegation_depth: 2,
        max_cost_usd: Some(5.0),
        max_runtime_seconds: Some(300),
        allow_self_update: false,
        require_approval_for_self_update: true,
        allowed_agents: vec![],
        allowed_tools: ToolSet::ReadWrite,
        allowed_skills: vec!["rust_debugging".to_string(), "test_writing".to_string()],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy::default(),
    };

    Agent {
        name: "coder".into(),
        description: "Implements approved software changes.".into(),
        pipeline,
        tools: ToolSet::ReadWrite,
        skills: SkillSet::with_skills(vec![
            "rust_debugging".to_string(),
            "test_writing".to_string(),
        ]),
        policy,
    }
}
