//! Fallback pipeline execution handler.
//!
//! When a step with a `FailureMode::Fallback(fallback_pipeline)` fails, the fallback
//! is executed as a replacement for the failed step — **not** as a deeper nesting level.
//!
//! This means the fallback inherits the failing step's delegation_depth unchanged,
//! budget, filesystem_policy, network_policy, and effective tool scope — preserving
//! the security context and preventing depth- or tool-scope-based laundries.

use super::PipelineRunner;
use super::PipelineError;
use crate::action::StepError;
use crate::agent::Agent;
use crate::audit::{AuditEntry, AuditEvent};
use crate::context::StepContext;
use crate::pipeline::Pipeline;
use crate::runner::types::PipelineResult;
use crate::skills::skill::SkillSet;
use chrono::Utc;
use serde_json;

impl PipelineRunner {
    /// Execute a fallback pipeline when a step fails.
    ///
    /// A fallback replaces the failed step *in place*, running under the same security
    /// context (same delegation_depth, budget, filesystem_policy, network_policy, and
    /// effective tool scope).
    ///
    /// # Arguments
    ///
    /// * `fallback_pipeline` - The pipeline to run as a fallback.
    /// * `original_error` - The error from the failed step.
    /// * `step_name` - Name of the step that failed.
    /// * `agent` - The agent executing the step.
    /// * `ctx` - The current step context (will be updated with fallback's budget).
    /// * `input` - The input to the fallback pipeline.
    ///
    /// # Returns
    ///
    /// - `Ok(PipelineResult)` if the fallback succeeds (with updated budget).
    /// - `Err(PipelineError::StepFailed)` if the fallback fails.
    pub(crate) async fn handle_fallback(
        &mut self,
        fallback_pipeline: &Pipeline,
        original_error: StepError,
        step_name: String,
        agent: &Agent,
        ctx: &mut StepContext,
        input: serde_json::Value,
    ) -> Result<PipelineResult, PipelineError> {
        self.audit_log.append(AuditEntry {
            timestamp: Utc::now(),
            pipeline_name: ctx.pipeline_name.clone(),
            step_name: step_name.clone(),
            event: AuditEvent::FallbackTriggered {
                step: step_name.clone(),
                reason: format!("{:?}", original_error),
            },
        });

        // A fallback pipeline is a substitute execution for the step that
        // just failed, so it MUST run under the same security context.
        //
        // This handler previously built a fresh `PipelineRunner` and called
        // the plain `run()` entry point with the unmodified `agent`, which
        // silently reset:
        //   * `delegation_depth` -> 0, so a delegated child sitting at
        //     `max_delegation_depth` could launder unlimited further
        //     delegation through its fallback pipeline
        //   * `budget`           -> fresh, restarting cost/call accounting
        //   * `filesystem_policy` -> re-resolved from the agent policy, so a
        //     `WorkspaceIsolation::TempDir` agent minted a SECOND temp dir
        //     instead of staying pinned to the parent's active sandbox
        //
        // Depth semantics: unlike `StepAction::SubPipeline` (which is a real
        // nesting boundary and therefore uses `delegation_depth + 1`), a
        // fallback replaces the failed step *in place* — it is the same
        // logical step retried a different way, not a level deeper. So it
        // inherits `ctx.delegation_depth` UNCHANGED. Containment still holds:
        // any delegation or SubPipeline *inside* the fallback increments from
        // that inherited depth, so a fallback at the cap cannot descend.
        let fallback_depth = ctx.delegation_depth;

        // Derive the fallback agent's policy from the PARENT's actual policy
        // so cost/step/runtime caps, allowed agents and skills all carry over.
        let mut fallback_policy = ctx.agent_policy.clone();

        // SECURITY (privilege escalation fix). Tool scope must be clamped to the
        // FAILED STEP's already-narrowed effective scope, not left at the agent
        // level. This handler previously cloned `ctx.agent_policy` verbatim and
        // justified it with "each fallback step intersects the agent policy with
        // its own declared `tools` during `run_internal`" — which conflates
        // widening with narrowing. That intersection only lets a fallback step
        // narrow further *from the agent default*; it cannot reconstruct a
        // restriction the failed step imposed. So a step scoped
        // `ToolSet::Deny(["x"])` whose fallback pipeline declared `ToolSet::Full`
        // computed `agent_policy ∩ Full` one level down and called `x` freely.
        //
        // A fallback is the same logical step retried a different way (see the
        // depth rationale above): if it inherits the step's depth and budget, it
        // must inherit the step's tool ceiling too. `ctx.allowed_tools` is that
        // ceiling (agent ∩ pipeline ∩ step ∩ skill) as of the moment of failure.
        //
        // Routed through the shared `child_policy` helper — the same one
        // `delegation::execute_delegation` and the `SubPipeline` handler in
        // `step_executor.rs` use — so these boundaries cannot drift apart again.
        super::child_policy::narrow_child_tool_scope(&mut fallback_policy, &ctx.allowed_tools);

        // Filesystem: inherit the parent's *resolved* policy.
        // `ctx.filesystem_policy.workspace_root` already points at the active
        // TempDir/Sandboxed root, so isolation is set to `None` to keep the
        // fallback inside that same sandbox rather than creating a new one.
        fallback_policy.filesystem_policy = ctx.filesystem_policy.clone();
        fallback_policy.filesystem_policy.workspace_isolation =
            crate::agent::WorkspaceIsolation::None;

        // Network: inherit the parent's actual policy, not a fresh default.
        fallback_policy.network_policy = ctx.network_policy.clone();

        let fallback_agent = Agent {
            name: agent.name.clone(),
            description: agent.description.clone(),
            pipeline: fallback_pipeline.clone(),
            tools: fallback_policy.allowed_tools.clone(),
            skills: SkillSet {
                skills: ctx.active_skills.clone(),
            },
            policy: fallback_policy,
            scorers: agent.scorers.clone(),
        };

        let mut fallback_runner = PipelineRunner {
            audit_log: crate::audit::AuditLog::new(),
            tool_registry: self.tool_registry.clone(),
            agent_registry: self.agent_registry.clone(),
            skill_registry: self.skill_registry.clone(),
            llm_client: self.llm_client.clone(),
            output_sink: self.output_sink.clone(),
            conversation_registry: self.conversation_registry.clone(),
            context_store: self.context_store.clone(),
            plugin_registry: self.plugin_registry.clone(),
            auto_title_llm: self.auto_title_llm.clone(),
            memory: self.memory.clone(),
            rate_limiter: self.rate_limiter.clone(),
        };

        // Run through the depth/budget-aware entry point — the same one
        // `execute_delegation` and the SubPipeline handler use. The parent's
        // spent budget goes in; the fallback's final budget comes back out on
        // the returned `PipelineResult`, keeping accounting continuous.
        let fallback_result = fallback_runner
            .run_with_delegation_depth_and_budget(
                fallback_pipeline,
                &fallback_agent,
                input,
                fallback_depth,
                ctx.parent_agent.clone().unwrap_or_else(|| ctx.agent_name.clone()),
                Some(ctx.budget.clone()),
            )
            .await;

        match fallback_result {
            Ok(result) => {
                // Budget continuity: the fallback's spend is the run's spend.
                ctx.budget = result.budget.clone();
                Ok(result)
            }
            Err(_) => Err(PipelineError::StepFailed {
                step: step_name,
                error: original_error,
            }),
        }
    }
}
