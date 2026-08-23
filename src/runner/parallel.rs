use super::step_exec::{
    emit_step_started, run_action, run_guard_in, run_post_action, step_tool_scope,
};
use super::PipelineRunner;
use crate::action::{StepAction, StepError};
use crate::context::StepContext;
use crate::pipeline::Pipeline;

/// Execute a batch of parallel steps.
///
/// Each step runs via the shared `step_exec` phases, so every `StepAction`
/// variant is supported and `guard_in` is evaluated for parallel steps too.
///
/// Concurrency strategy: `StepAction::Custom` closures are synchronous and
/// typically block the calling thread (`std::thread::sleep`, blocking I/O), so
/// they are dispatched to `tokio::task::spawn_blocking` and awaited together,
/// giving real OS-thread parallelism. `Custom` is declared `Send + Sync` (see
/// `action.rs::StepAction::Custom`), so this is sound.
///
/// Non-`Custom` variants are `async` but execute sequentially because
/// `run_action`'s future is not `Send` (it holds `&mut PipelineRunner` across
/// awaits). Spawning them would require moving `&mut PipelineRunner` into
/// multiple concurrent tasks, which is unsound. They await sequentially on the
/// current task while `Custom` blocking tasks run concurrently on their own threads.
///
/// Each step gets an isolated context clone to prevent cross-step interference.
/// Results merge into the primary context afterwards (last-writer-wins), along
/// with each step's budget spend and tool/shell-command records — see
/// `merge_parallel_step_deltas`.
///
/// # Arguments
/// - `runner`: mutable pipeline runner (for audit log and action execution)
/// - `pipeline`: the pipeline being executed (for step lookup)
/// - `ctx`: mutable context (merged with batch results post-execution)
/// - `batch`: step indices to execute as one parallel batch
/// - `agent_tools`: the agent policy's tool scope, intersected with each step's scope
///
/// # Returns
/// Ok(Vec of (step_name, StepResult)) if all steps succeed, Err(PipelineError) on first failure.
pub(crate) async fn execute_parallel_batch(
    runner: &mut PipelineRunner,
    pipeline: &Pipeline,
    ctx: &mut StepContext,
    batch: &[usize],
    agent_tools: &crate::toolset::ToolSet,
) -> Result<Vec<(String, crate::context::StepResult)>, super::PipelineError> {
    // Baseline snapshot: every per-step context below is cloned from `ctx`
    // *before* any step mutates it, so all clones share these starting values.
    // Each step's contribution is therefore (its final state - this baseline).
    let baseline = BatchBaseline {
        spent_usd: ctx.budget.spent_usd,
        llm_calls_used: ctx.budget.llm_calls_used,
        tool_calls_used: ctx.budget.tool_calls_used,
        tools_used_len: ctx.tools_used.len(),
        commands_executed_len: ctx.commands_executed.len(),
    };

    // Phase 1: build each step's isolated context and evaluate guard_in.
    // guard_in runs before any action is dispatched, so a failing guard blocks
    // its step's action from ever starting.
    let mut prepared = Vec::with_capacity(batch.len());
    for &step_idx in batch {
        let step = pipeline.steps[step_idx].clone();

         let mut step_ctx = ctx.clone();
         step_ctx.step_name = step.name.clone();
         step_ctx.input = ctx.input.clone();
         // Effective tool scope = agent policy ∩ step scope, via the same helper
         // the sequential path uses. Intersecting the inherited context scope
         // here instead would bypass agent-level policy (see `step_tool_scope`).
         step_ctx.allowed_tools = step_tool_scope(agent_tools, &step);
         step_ctx.injection_protection = step.injection_protection.clone();

        emit_step_started(runner, &step, &step_ctx);
        if let Err(e) = run_guard_in(runner, &step, &step_ctx).await {
            return Err(super::PipelineError::StepFailed {
                step: step.name.clone(),
                error: e,
            });
        }

        prepared.push((step, step_ctx));
    }

    // Phase 2: run the actions concurrently.
    // Blocking `Custom` closures go to the blocking thread pool; async variants
    // are polled together by `join_all`.
    let mut blocking = Vec::new();
    let mut async_idx = Vec::new();
    for (i, (step, step_ctx)) in prepared.iter().enumerate() {
        if let StepAction::Custom(f) = &step.action {
            let f = f.clone();
            let call_ctx = step_ctx.clone();
            blocking.push((
                i,
                tokio::task::spawn_blocking(move || f(&call_ctx)),
            ));
        } else {
            async_idx.push(i);
        }
    }

    let mut outputs: Vec<Option<Result<crate::action::StepOutput, StepError>>> =
        (0..prepared.len()).map(|_| None).collect();

    // Drive async actions sequentially while the blocking tasks run on their threads.
    for i in async_idx {
        let (step, step_ctx) = &mut prepared[i];
        outputs[i] = Some(run_action(runner, step, step_ctx).await);
    }

    for (i, handle) in blocking {
        let joined = handle.await.unwrap_or_else(|e| {
            Err(StepError::ActionFailed {
                reason: format!("parallel step task failed: {e}"),
            })
        });
        outputs[i] = Some(joined);
    }

    // Phase 3: post-action (guard_out, verdict, StepCompleted) in batch order,
    // keeping audit events deterministic regardless of completion order.
    // A failure is recorded rather than returned immediately: actions have
    // already run by this point, so their budget spend and executed commands
    // must still be merged back before the batch aborts. Returning early would
    // discard exactly the evidence the guards exist to see.
    let mut batch_results = Vec::with_capacity(prepared.len());
    let mut failure: Option<super::PipelineError> = None;
    for (i, (step, step_ctx)) in prepared.iter_mut().enumerate() {
        let action_result = outputs[i]
            .take()
            .ok_or_else(|| super::PipelineError::StepFailed {
                step: step.name.clone(),
                error: StepError::ActionFailed {
                    reason: "missing action result (internal: slot was None)".to_string(),
                },
            })?;

        let output = match action_result {
            Ok(output) => output,
            Err(e) => {
                runner.audit_log.append(crate::audit::AuditEntry {
                    timestamp: chrono::Utc::now(),
                    pipeline_name: step_ctx.pipeline_name.clone(),
                    step_name: step.name.clone(),
                    event: crate::audit::AuditEvent::StepFailed {
                        error: format!("{:?}", e),
                    },
                });
                failure = Some(super::PipelineError::StepFailed {
                    step: step.name.clone(),
                    error: e,
                });
                break;
            }
        };

        match run_post_action(runner, step, step_ctx, output).await {
            Ok(output) => {
                let sr = crate::context::StepResult {
                    step_name: step.name.clone(),
                    output,
                    verdict_passed: true,
                    error: None,
                };
                batch_results.push((step.name.clone(), sr));
            }
            Err(e) => {
                failure = Some(super::PipelineError::StepFailed {
                    step: step.name.clone(),
                    error: e.into(),
                });
                break;
            }
        }
    }

    // Merge batch results into primary context (last-writer-wins per invariant #9)
    for (step_name, step_result) in batch_results.iter() {
        ctx.step_results
            .insert(step_name.clone(), step_result.clone());
    }

    // Merge per-step budget spend and tool/command records back into the parent.
    merge_parallel_step_deltas(ctx, &baseline, prepared.iter().map(|(_, c)| c));

    match failure {
        Some(e) => Err(e),
        None => Ok(batch_results),
    }
}

/// The parent context's accounting state at the moment the batch began.
struct BatchBaseline {
    spent_usd: f64,
    llm_calls_used: u32,
    tool_calls_used: u32,
    tools_used_len: usize,
    commands_executed_len: usize,
}

/// Fold each parallel step's accounting delta back into the parent context.
///
/// Parallel steps run on isolated context clones so they cannot interfere with
/// each other, but that isolation also means their spend and their tool/shell
/// records die with the clone unless merged here. Without this, `MaxToolCalls`
/// / `MaxCostUsd` see zero spend and `ShellCommandAllowlist` /
/// `ShellCommandDenylist` see zero commands for anything a parallel step did.
///
/// Every clone starts from the same `baseline`, so a step's own contribution is
/// its final value minus that baseline. Counters SUM across steps; the
/// `tools_used` / `commands_executed` vectors CONCATENATE in batch order (not
/// completion order) so the merged result is deterministic.
///
/// Counter deltas use `saturating_sub` and clamp cost at zero: a step context is
/// only ever appended to, so a negative delta is impossible, and clamping keeps
/// a future regression from silently *crediting* budget back.
fn merge_parallel_step_deltas<'a>(
    ctx: &mut StepContext,
    baseline: &BatchBaseline,
    step_ctxs: impl Iterator<Item = &'a StepContext>,
) {
    let mut spent_delta = 0.0_f64;
    let mut llm_delta = 0_u32;
    let mut tool_delta = 0_u32;

    for step_ctx in step_ctxs {
        spent_delta += (step_ctx.budget.spent_usd - baseline.spent_usd).max(0.0);
        llm_delta += step_ctx
            .budget
            .llm_calls_used
            .saturating_sub(baseline.llm_calls_used);
        tool_delta += step_ctx
            .budget
            .tool_calls_used
            .saturating_sub(baseline.tool_calls_used);

        if let Some(added) = step_ctx.tools_used.get(baseline.tools_used_len..) {
            ctx.tools_used.extend_from_slice(added);
        }
        if let Some(added) = step_ctx
            .commands_executed
            .get(baseline.commands_executed_len..)
        {
            ctx.commands_executed.extend_from_slice(added);
        }
    }

    ctx.budget.spent_usd += spent_delta;
    ctx.budget.llm_calls_used += llm_delta;
    ctx.budget.tool_calls_used += tool_delta;
    if let Some(remaining) = ctx.budget.remaining_usd.as_mut() {
        *remaining -= spent_delta;
    }
}
