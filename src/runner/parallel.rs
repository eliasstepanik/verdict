use super::step_exec::{emit_step_started, run_action, run_guard_in, run_post_action};
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
/// Results merge into the primary context afterwards (last-writer-wins).
///
/// # Arguments
/// - `runner`: mutable pipeline runner (for audit log and action execution)
/// - `pipeline`: the pipeline being executed (for step lookup)
/// - `ctx`: mutable context (merged with batch results post-execution)
/// - `batch`: step indices to execute as one parallel batch
///
/// # Returns
/// Ok(Vec of (step_name, StepResult)) if all steps succeed, Err(PipelineError) on first failure.
pub(crate) async fn execute_parallel_batch(
    runner: &mut PipelineRunner,
    pipeline: &Pipeline,
    ctx: &mut StepContext,
    batch: &[usize],
) -> Result<Vec<(String, crate::context::StepResult)>, super::PipelineError> {
    // Phase 1: build each step's isolated context and evaluate guard_in.
    // guard_in runs before any action is dispatched, so a failing guard blocks
    // its step's action from ever starting.
    let mut prepared = Vec::with_capacity(batch.len());
    for &step_idx in batch {
        let step = pipeline.steps[step_idx].clone();

        let mut step_ctx = ctx.clone();
        step_ctx.step_name = step.name.clone();
        step_ctx.input = ctx.input.clone();
        step_ctx.allowed_tools = crate::toolset::ToolSet::Intersection(
            Box::new(step_ctx.allowed_tools.clone()),
            Box::new(step.tools.clone()),
        );

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
    let mut batch_results = Vec::with_capacity(prepared.len());
    for (i, (step, step_ctx)) in prepared.iter_mut().enumerate() {
        let action_result = outputs[i]
            .take()
            .expect("every prepared step is assigned an action result");

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
                return Err(super::PipelineError::StepFailed {
                    step: step.name.clone(),
                    error: e,
                });
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
                return Err(super::PipelineError::StepFailed {
                    step: step.name.clone(),
                    error: e.into(),
                });
            }
        }
    }

    // Merge batch results into primary context (last-writer-wins per invariant #9)
    for (step_name, step_result) in batch_results.iter() {
        ctx.step_results
            .insert(step_name.clone(), step_result.clone());
    }

    Ok(batch_results)
}
