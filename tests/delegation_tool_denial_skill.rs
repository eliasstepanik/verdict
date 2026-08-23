//! Probe: NESTED COMPOSITION — UseSkill -> SubPipeline -> DelegateAgent
//!
//! Two narrowing mechanisms were fixed independently:
//!   1. `use_skill.rs` — `ctx.allowed_tools = ctx.allowed_tools ∩ skill.allowed_tools`
//!   2. `child_policy.rs::narrow_child_tool_scope` — clamps a delegated child's
//!      `policy.allowed_tools` to the parent's effective ceiling.
//!
//! Neither proves the pair COMPOSES. A `SkillMode::Pipeline` skill runs its
//! pipeline as a `SubPipeline`; if that pipeline itself contains a
//! `DelegateAgent` step, the skill's restriction must survive TWO further
//! boundary crossings (skill -> sub-pipeline -> delegated agent) before it
//! reaches the tool-call enforcement point in `tool_executor.rs`.
//!
//! Observation is again a real side effect (the `AtomicBool` tripwire), not a
//! string match on an error.

mod common;

use serde_json::json;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use verdict::prelude::*;

use common::delegation::{agent, pipeline, step, tripwire_tool};

/// A skill whose pipeline delegates further, to `level2`. `skill_tools` is the
/// restriction under test; the sub-pipeline step itself asks for `ToolSet::Full`
/// so only an inherited restriction can bind it.
fn delegating_skill(skill_tools: ToolSet) -> Skill {
    Skill {
        name: "restricted_skill".into(),
        description: "Skill whose pipeline delegates to another agent.".into(),
        instructions: "unused in Pipeline mode".into(),
        allowed_tools: skill_tools,
        required_guards: vec![],
        pipeline: Some(pipeline(
            "restricted_skill_pipeline",
            vec![step(
                "delegate_from_skill",
                StepAction::DelegateAgent {
                    agent: "level2".into(),
                    input: json!({}),
                    expected_output_schema: None,
                    delegation_policy: DelegationPolicy::default(),
                    detached: false,
                },
                ToolSet::Full,
            )],
        )),
        examples: vec![],
        eval: None,
    }
}

/// Builds: level1 (scope = `parent_tools`)
///   --UseSkill(restricted_skill, scope = `skill_tools`)-->
///     --SubPipeline--> --DelegateAgent--> level2 (own policy wide open)
///       --ToolCall--> test.denied_tool
///
/// Returns whether the tool actually executed, plus the pipeline outcome.
async fn run_nested_chain(
    parent_tools: ToolSet,
    skill_tools: ToolSet,
) -> (bool, Result<PipelineResult, PipelineError>) {
    let flag = Arc::new(AtomicBool::new(false));

    let mut tools = ToolRegistry::with_builtins();
    tools.register(tripwire_tool(flag.clone()));

    // Innermost agent: its OWN policy permits everything.
    let level2 = agent(
        "level2",
        pipeline(
            "level2_pipeline",
            vec![step(
                "call_denied_tool",
                StepAction::ToolCall {
                    tool: "test.denied_tool".into(),
                    args: json!({}),
                },
                ToolSet::Full,
            )],
        ),
        ToolSet::Full,
    );

    let mut agents = AgentRegistry::new();
    agents.register(level2);

    let mut skills = SkillRegistry::new();
    skills.register(delegating_skill(skill_tools));

    let level1_pipeline = pipeline(
        "level1_pipeline",
        vec![step(
            "use_skill",
            StepAction::UseSkill {
                skill: "restricted_skill".into(),
                input: json!({}),
                mode: SkillMode::Pipeline,
            },
            ToolSet::Full,
        )],
    );
    let level1 = agent("level1", level1_pipeline.clone(), parent_tools);

    let mut runner = PipelineRunner::with_registries(Arc::new(tools), Arc::new(agents));
    runner.skill_registry = Arc::new(skills);

    let result = runner.run(&level1_pipeline, &level1, json!({})).await;

    (flag.load(std::sync::atomic::Ordering::SeqCst), result)
}

/// THE NESTED PROBE (skill-side restriction). The SKILL denies the tool; every
/// other layer (parent agent, step, delegated agent) is wide open. The denial
/// must survive UseSkill -> SubPipeline -> DelegateAgent.
#[tokio::test]
async fn skill_deny_binds_agent_delegated_from_inside_skill_pipeline() {
    let (tool_ran, result) = run_nested_chain(
        ToolSet::Full,
        ToolSet::Deny(vec!["test.denied_tool".into()]),
    )
    .await;

    assert!(
        !tool_ran,
        "ESCALATION: 'test.denied_tool' is denied by skill 'restricted_skill', yet it \
         executed in agent 'level2' delegated from inside that skill's pipeline \
         (UseSkill -> SubPipeline -> DelegateAgent). Pipeline result: {result:?}"
    );
}

/// THE NESTED PROBE (parent-side restriction). The PARENT AGENT denies the tool
/// and the skill is wide open — the reverse operand ordering. A skill invocation
/// must not launder a parent denial away through the extra delegation hop.
#[tokio::test]
async fn parent_deny_survives_skill_pipeline_delegation() {
    let (tool_ran, result) = run_nested_chain(
        ToolSet::Deny(vec!["test.denied_tool".into()]),
        ToolSet::Full,
    )
    .await;

    assert!(
        !tool_ran,
        "ESCALATION: 'test.denied_tool' is denied by level1's policy, yet it executed \
         in agent 'level2' delegated from inside a UseSkill pipeline. Pipeline result: {result:?}"
    );
}

/// Positive control for the nested chain: with NO denial anywhere, the exact
/// same three-boundary chain must reach and run the tool. Without this, both
/// nested denial tests could "pass" merely because the chain never got that far.
#[tokio::test]
async fn positive_control_nested_chain_reaches_tool() {
    let (tool_ran, result) = run_nested_chain(ToolSet::Full, ToolSet::Full).await;

    assert!(
        result.as_ref().map(|r| r.success).unwrap_or(false),
        "harness broken: unrestricted UseSkill->SubPipeline->DelegateAgent chain \
         should succeed, got {result:?}"
    );
    assert!(
        tool_ran,
        "harness broken: unrestricted nested chain never reached the tool call, so the \
         nested denial tests prove nothing. Pipeline result: {result:?}"
    );
}
