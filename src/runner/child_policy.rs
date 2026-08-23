//! Child-boundary policy narrowing — the single source of truth for the rule
//! "a child execution can never hold a BROADER tool scope than its parent".
//!
//! Every delegation boundary (`StepAction::SubPipeline`, `StepAction::DelegateAgent`)
//! spawns a nested `run_internal`, which recomputes each inner step's effective
//! scope from the *child's* `policy.allowed_tools` (`execution.rs`, via
//! `step_exec::step_tool_scope`). If the boundary hands the child a policy whose
//! `allowed_tools` was not first clamped to the parent's already-narrowed scope,
//! the parent's restriction is simply absent from that computation and a tool the
//! parent denied becomes callable one hop down.
//!
//! That is exactly how `DelegateAgent` escalated: `DelegationPolicy::inherit_tool_scope`
//! only selects WHICH `ToolRegistry` the child receives (permission-*widening*); it
//! never narrowed the child's allowed-tools SET (permission-*narrowing*). The two
//! are independent concerns and were conflated. `SubPipeline` had been fixed in
//! isolation, so the two boundaries diverged.
//!
//! Both boundaries now call [`narrow_child_tool_scope`] so they cannot diverge again.

use crate::agent::AgentPolicy;
use crate::toolset::ToolSet;

/// Clamp a child execution's tool scope to its parent's effective ceiling.
///
/// `child_policy.allowed_tools` becomes `child_declared ∩ parent_effective`, so the
/// child may only ever *narrow* what the parent already permits — never widen it.
///
/// `parent_effective` must be `ctx.allowed_tools`: the parent's already-narrowed
/// scope for the delegating step (agent ∩ pipeline ∩ step ∩ skill), not the
/// parent's agent-level default. Passing the agent-level set would re-widen the
/// child back past the step's own restriction.
///
/// The intersection is deliberately lazy (`ToolSet::Intersection`) rather than an
/// eagerly-flattened name list: `ToolSet` variants such as `Deny`, `ReadOnly` and
/// `FromSkill` are open sets that cannot be enumerated without the registry, and
/// `ToolSet::contains{,_with_skill_registry}` already evaluates `Intersection`
/// as short-circuit conjunction at the enforcement point.
///
/// This is idempotent in effect for boundaries whose child scope is already derived
/// from the parent (`SubPipeline`), and load-bearing for boundaries whose child
/// declares its own independent policy (`DelegateAgent`).
pub(crate) fn narrow_child_tool_scope(child_policy: &mut AgentPolicy, parent_effective: &ToolSet) {
    child_policy.allowed_tools = ToolSet::Intersection(
        Box::new(child_policy.allowed_tools.clone()),
        Box::new(parent_effective.clone()),
    );
}
