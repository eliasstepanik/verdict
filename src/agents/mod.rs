//! Built-in agents for the Verdict framework
//!
//! This module provides six specialized agents:
//! - `planner_agent`: Produces structured execution plans
//! - `coder_agent`: Implements approved software changes
//! - `reviewer_agent`: Reviews code changes for quality and safety
//! - `debugger_agent`: Diagnoses and fixes compile/test failures
//! - `reflector_agent`: Analyzes agent performance
//! - `orchestrator_agent`: Delegates work to specialized agents

use crate::action::{DelegationPolicy, MemoryIsolation, StepAction};
use crate::agent::{Agent, AgentPolicy, FilesystemPolicy, NetworkPolicy};
use crate::guards::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;
use serde_json::json;

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
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
        scorers: Vec::new(),
    }
}

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
            user: "Plan: {plan}\n\nTask: {task}\n\nImplement the changes. Produce a diff.".into(),
            model: None,
            conversation_id: None,
            append_to_history: true,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadWrite,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: Vec::new(),
        parallel: false,
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
        scorers: Vec::new(),
    }
}

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
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
        scorers: Vec::new(),
    }
}

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
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
        scorers: Vec::new(),
    }
}

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
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
        scorers: Vec::new(),
    }
}

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
                on_delegation_start: None,
                on_delegation_complete: None,
                on_iteration_complete: None,
                message_filter: None,
                memory_isolation: MemoryIsolation::Isolated,
            },
            detached: false,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: Vec::new(),
        parallel: false,
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
                on_delegation_start: None,
                on_delegation_complete: None,
                on_iteration_complete: None,
                message_filter: None,
                memory_isolation: MemoryIsolation::Isolated,
            },
            detached: false,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: vec!["orchestrate".into()],
        parallel: false,
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
                on_delegation_start: None,
                on_delegation_complete: None,
                on_iteration_complete: None,
                message_filter: None,
                memory_isolation: MemoryIsolation::Isolated,
            },
            detached: false,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::Strict,
        output_schema: None,
        dependencies: vec!["implement".into()],
        parallel: false,
        input_processors: Vec::new(),
        output_processors: Vec::new(),
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
        scorers: Vec::new(),
    }
}
