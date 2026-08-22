// Step action handlers: SubPipeline, LoopUntil, Branch, UseSkill.
// UseSkill handler and helpers extracted to use_skill.rs for modularity.

use crate::runner::PipelineRunner;

impl PipelineRunner {
    /// Handle SubPipeline action.
    /// 
    /// A sub-pipeline is a delegation boundary and MUST inherit the parent's security context
    /// exactly like `delegation::execute_delegation` does. This handler implements the
    /// C2/C3 security fixes: inherits parent's filesystem_policy/network_policy/delegation_depth
    /// instead of resetting to fresh defaults (which previously allowed a sandboxed sub-pipeline
    /// to write files to the real repo and bypass max_delegation_depth).
    pub(crate) async fn handle_subpipeline(
        &mut self,
        pipeline: &std::boxed::Box<crate::pipeline::Pipeline>,
        ctx: &mut crate::context::StepContext,
    ) -> Result<crate::action::StepOutput, crate::action::StepError> {
        use crate::skills::skill::SkillSet;

        // A sub-pipeline is a delegation boundary and MUST inherit the parent's
        // security context exactly like `delegation::execute_delegation` does.
        //
        // This handler previously built an `AgentPolicy::default()` and called
        // the plain `run()` entry point, which silently reset:
        //   * `filesystem_policy`  -> fresh `current_dir()` root, dropping
        //     `WorkspaceIsolation::TempDir`/`Sandboxed` (writes escaped to the real repo)
        //   * `network_policy`     -> `DenyAll` (fail-closed, but still divergent)
        //   * `delegation_depth`   -> 0, letting a SubPipeline wrapper bypass
        //     `max_delegation_depth` / `Guard::MaxDelegationDepth` entirely
        //   * `budget`             -> fresh, resetting cost/call accounting
        //
        // Everything below derives from the parent context instead.
        let child_depth = ctx.delegation_depth + 1;

        // Enforce the parent's delegation depth cap on this boundary, mirroring
        // the depth check `execute_delegation` performs before recursing.
        if child_depth > ctx.agent_policy.max_delegation_depth {
            return Err(crate::action::StepError::ActionFailed {
                reason: format!(
                    "SubPipeline delegation depth {} exceeds max {}",
                    child_depth, ctx.agent_policy.max_delegation_depth
                ),
            });
        }

        // Create a child runner with shared registries
        let mut child_runner = crate::runner::PipelineRunner {
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
        };

        // Derive the sub-agent's policy from the PARENT's actual policy so that
        // cost/step/runtime caps, allowed agents and skills all carry over.
        let mut policy = ctx.agent_policy.clone();

        // Tool scope: the already-narrowed effective scope from the parent context
        // (agent ∩ pipeline ∩ step ∩ skill), so inner steps intersect against the
        // narrowed set rather than the agent-level default.
        policy.allowed_tools = ctx.allowed_tools.clone();

        // Filesystem: inherit the parent's *resolved* policy. `ctx.filesystem_policy`
        // already has `workspace_root` rewritten to the active TempDir/Sandboxed root
        // by `run_internal`, so isolation is set to `None` here to keep the child
        // pinned to that same sandbox instead of minting a second, unrelated temp dir
        // (which would hide the parent's files from the sub-pipeline).
        policy.filesystem_policy = ctx.filesystem_policy.clone();
        policy.filesystem_policy.workspace_isolation =
            crate::agent::WorkspaceIsolation::None;

        // Network: inherit the parent's actual policy, not a fresh DenyAll.
        policy.network_policy = ctx.network_policy.clone();

        let agent = crate::agent::Agent {
            name: ctx.agent_name.clone(),
            description: "Child pipeline agent".into(),
            pipeline: pipeline.as_ref().clone(),
            tools: ctx.allowed_tools.clone(),
            skills: SkillSet {
                skills: ctx.active_skills.clone(),
            },
            policy,
            scorers: Vec::new(),
        };

        // Run the child pipeline through the depth/budget-aware entry point —
        // the same one `execute_delegation` uses.
        let result = child_runner
            .run_with_delegation_depth_and_budget(
                pipeline,
                &agent,
                ctx.input.clone(),
                child_depth,
                ctx.agent_name.clone(),
                Some(ctx.budget.clone()),
            )
            .await
            .map_err(|e| crate::action::StepError::ActionFailed {
                reason: format!("SubPipeline failed: {}", e),
            })?;

        // Propagate the child's budget back so cost/call accounting is continuous.
        ctx.budget = result.budget.clone();

        // Merge the child's audit entries into the parent log so the sub-pipeline
        // is not invisible to auditing.
        for entry in result.audit_log.entries() {
            self.audit_log.append(entry.clone());
        }

        // Get the LAST step's output deterministically by finding the last step in
        // the pipeline and looking it up by name (not by HashMap iteration order).
        let output = pipeline
            .steps
            .last()
            .and_then(|last_step| result.step_results.get(&last_step.name))
            .map(|sr| sr.output.clone())
            .unwrap_or_else(|| crate::action::StepOutput::new(String::new()));

        Ok(output)
    }

    /// Handle LoopUntil action.
    /// Executes a body action repeatedly until a condition guard passes or max_iterations reached.
    pub(crate) async fn handle_loop_until(
        &mut self,
        body: &std::boxed::Box<crate::action::StepAction>,
        condition: &crate::guards::Guard,
        max_iterations: &u32,
        on_iteration_failure: &crate::action::IterationFailureMode,
        ctx: &mut crate::context::StepContext,
    ) -> Result<crate::action::StepOutput, crate::action::StepError> {
        use crate::guards::GuardEngine;

        for iteration in 0u32..*max_iterations {
            // Execute the body
            let body_result = self.execute_action(body, ctx).await;

            match body_result {
                Ok(output) => {
                    ctx.output = Some(output);
                }
                Err(e) => {
                    match on_iteration_failure {
                        crate::action::IterationFailureMode::Retry => {
                            // Continue to next iteration
                            continue;
                        }
                        crate::action::IterationFailureMode::Skip => {
                            // Skip to next iteration without error
                            continue;
                        }
                        crate::action::IterationFailureMode::Abort => {
                            // Fail the entire loop
                            return Err(e);
                        }
                    }
                }
            }

            // Check the condition guard
            match GuardEngine::evaluate(condition, ctx).await {
                Ok(()) => {
                    // Condition passed, exit the loop
                    break;
                }
                Err(_) => {
                    // Condition failed, continue looping
                    if iteration + 1 >= *max_iterations {
                        break; // Max iterations reached — exit loop normally
                    }
                }
            }
        }

        Ok(ctx
            .output
            .clone()
            .unwrap_or_else(|| crate::action::StepOutput::new(String::new())))
    }

    /// Handle Branch action.
    /// Dispatches to if_true or if_false branch based on condition string presence in output.
    pub(crate) async fn handle_branch(
        &mut self,
        condition: &str,
        if_true: &crate::action::StepAction,
        if_false: &Option<Box<crate::action::StepAction>>,
        ctx: &mut crate::context::StepContext,
    ) -> Result<crate::action::StepOutput, crate::action::StepError> {
        let output_str = ctx
            .output
            .as_ref()
            .map(|o| o.raw.clone())
            .unwrap_or_default();

        if output_str.contains(condition) {
            self.execute_action(if_true, ctx).await
        } else if let Some(false_action) = if_false {
            self.execute_action(false_action, ctx).await
        } else {
            // No false branch, return empty output
            Ok(crate::action::StepOutput::new(String::new()))
        }
    }
}
