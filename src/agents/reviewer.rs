//! Reviewer agent: reviews code changes for quality and safety

use crate::action::StepAction;
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guard::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Creates a reviewer agent that reviews code changes for quality and safety.
///
/// The reviewer agent is read-only and cannot modify code. It analyzes
/// diffs and provides approval/rejection decisions based on quality criteria.
pub fn reviewer_agent() -> Agent {
    let step = AgentStep {
        name: "review".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "You are a code review agent. Review the changes thoroughly.".into(),
            user: "Task: {task}\n\nDiff: {diff}\n\nReview for: correctness, security, quality. Output: approval_status, issues, required_fixes, risk_rating.".into(),
            model: None,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::None,
        output_schema: None,
    };

    let pipeline = Pipeline {
        name: "reviewer_pipeline".into(),
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
        allowed_skills: vec!["code_review".to_string()],
        network_policy: NetworkPolicy::DenyAll,
        filesystem_policy: FilesystemPolicy::default(),
    };

    Agent {
        name: "reviewer".into(),
        description: "Reviews code changes for quality, safety, and correctness.".into(),
        pipeline,
        tools: ToolSet::ReadOnly,
        skills: SkillSet::with_skills(vec!["code_review".to_string()]),
        policy,
    }
}
