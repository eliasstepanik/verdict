//! Orchestrator agent: delegates work to specialized agents

use crate::action::{StepAction, DelegationPolicy};
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guards::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;
use serde_json::json;

/// Creates an orchestrator agent that delegates work to specialized agents.
///
/// The orchestrator agent is read-only and coordinates the work of other agents
/// (planner, coder, reviewer, debugger, reflector) to achieve user goals.
pub fn orchestrator_agent() -> Agent {
    let plan_step = AgentStep {
        name: "orchestrate".into(),
        guard_in: Guard::None,
        action: StepAction::DelegateAgent {
            agent: "planner".into(),
            input: json!({ "task": "{input}" }),
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth: 2,
                allowed_agents: vec![],
                require_output_schema: false,
                inherit_tool_scope: true,
                inherit_budget: true,
                require_user_approval: false,
            },
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: Vec::new(),
        parallel: false,
    };

    let implement_step = AgentStep {
        name: "implement".into(),
        guard_in: Guard::StepPassed("orchestrate".into()),
        action: StepAction::DelegateAgent {
            agent: "coder".into(),
            input: json!({ "plan": "{orchestrate}", "task": "{input}" }),
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth: 2,
                allowed_agents: vec![],
                require_output_schema: false,
                inherit_tool_scope: true,
                inherit_budget: true,
                require_user_approval: false,
            },
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: vec!["orchestrate".into()],
        parallel: false,
    };

    let review_step = AgentStep {
        name: "review".into(),
        guard_in: Guard::StepPassed("implement".into()),
        action: StepAction::DelegateAgent {
            agent: "reviewer".into(),
            input: json!({ "code": "{implement}", "task": "{input}" }),
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth: 2,
                allowed_agents: vec![],
                require_output_schema: false,
                inherit_tool_scope: true,
                inherit_budget: true,
                require_user_approval: false,
            },
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: vec!["implement".into()],
        parallel: false,
    };

    let pipeline = Pipeline {
        name: "orchestrator_pipeline".into(),
        steps: vec![plan_step, implement_step, review_step],
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
