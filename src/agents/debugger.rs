//! Debugger agent: diagnoses and fixes compile and test failures

use crate::action::StepAction;
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guard::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Creates a debugger agent that diagnoses and fixes compile/test failures.
///
/// The debugger agent has read-write access to tools, allowing it to
/// modify code and run tests to verify fixes.
pub fn debugger_agent() -> Agent {
    let step = AgentStep {
        name: "debug".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a debugging agent. Diagnose and fix failures.".into(),
            user: "Failing command: {command}\n\nError output: {error}\n\nChanged files: {files}\n\nProvide: root_cause, patch, expected_test_result.".into(),
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
        name: "debugger_pipeline".into(),
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
        allowed_tools: ToolSet::ReadWrite,
        allowed_skills: vec!["rust_debugging".to_string()],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy::default(),
    };

    Agent {
        name: "debugger".into(),
        description: "Diagnoses and fixes compile and test failures.".into(),
        pipeline,
        tools: ToolSet::ReadWrite,
        skills: SkillSet::with_skills(vec!["rust_debugging".to_string()]),
        policy,
    }
}
