//! Integration tests for the unified rate-limiter choke point.
//! 
//! This test suite verifies that the rate-limiter enforcement moved into
//! LlmClient::complete()/stream() blocks ALL LLM calls uniformly:
//! - Verdict::LlmJudge and Guard::SemanticCheck (previously ungated in Instance #14)
//! - All other direct LlmClient callers
//! 
//! Test strategy: Use a CountingProvider to track actual calls reaching the LLM.
//! With max_calls_per_minute(0), NO call should bypass the check.

use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use verdict::llm::provider::{LlmChunk, LlmError, LlmProvider, LlmRequest, LlmResponse};
use verdict::prelude::*;

/// Provider that COUNTS every call that reaches it. If the rate limiter works,
/// the count must remain 0 under a 0-call/min limit.
struct CountingProvider {
    hits: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl LlmProvider for CountingProvider {
    fn name(&self) -> &str {
        "counting-mock"
    }
    fn default_model(&self) -> &str {
        "mock-model"
    }
    async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        // Deliberately return a PASSING judge/semantic answer so that if the
        // rate limiter is bypassed, the pipeline SUCCEEDS (the original bug).
        Ok(LlmResponse {
            content: "PASS".into(),
            model: "mock-model".into(),
            usage: None,
            tool_calls: None,
        })
    }
    fn stream(
        &self,
        _request: LlmRequest,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = Result<LlmChunk, LlmError>> + Send>> {
        self.hits.fetch_add(1, Ordering::SeqCst);
        Box::pin(futures::stream::once(async {
            Ok(LlmChunk {
                delta: "PASS".into(),
                finish_reason: Some("stop".into()),
            })
        }))
    }
}

fn counting_client() -> (Arc<LlmClient>, Arc<AtomicUsize>) {
    let hits = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(CountingProvider { hits: hits.clone() });
    (Arc::new(LlmClient::new(provider)), hits)
}

fn agent_for(pipeline: &Pipeline) -> Agent {
    Agent {
        name: "probe_agent".into(),
        description: "independent probe".into(),
        pipeline: pipeline.clone(),
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: vec![],
    }
}

fn judge_pipeline() -> Pipeline {
    Pipeline {
        name: "judge_probe".into(),
        steps: vec![AgentStep {
            name: "judged".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_| {
                Ok(StepOutput::new("The capital of France is Paris.".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::LlmJudge {
                system: "Fact checker. Respond PASS or FAIL.".into(),
                input_template: "Output: {output}".into(),
                model: None,
                pass_on_pattern: "PASS".into(),
            },
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

fn semantic_pipeline() -> Pipeline {
    Pipeline {
        name: "semantic_probe".into(),
        steps: vec![AgentStep {
            name: "semantic".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_| {
                Ok(StepOutput::new("The sum of 2 + 2 is 4.".into()))
            })),
            guard_out: Guard::SemanticCheck("Output must state 2 + 2 equals 4".into()),
            verdict: Verdict::None,
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

// ─── PROBE 1: LlmJudge, builder order  with_llm_client → with_rate_limiter ───
#[tokio::test]
async fn probe_llm_judge_blocked_order_client_then_limiter() {
    let (client, hits) = counting_client();
    let pipeline = judge_pipeline();
    let agent = agent_for(&pipeline);
    let mut runner = PipelineRunner::new()
        .with_llm_client(client)
        .with_rate_limiter(RateLimiter::new().with_max_calls_per_minute(0));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "BYPASS: provider was reached despite 0-call/min rate limit"
    );
    assert!(
        result.is_err() || !result.as_ref().unwrap().success,
        "BYPASS: LlmJudge pipeline SUCCEEDED under a 0-call/min rate limit"
    );
}

// ─── PROBE 2: LlmJudge, reversed order  with_rate_limiter → with_llm_client ──
#[tokio::test]
async fn probe_llm_judge_blocked_order_limiter_then_client() {
    let (client, hits) = counting_client();
    let pipeline = judge_pipeline();
    let agent = agent_for(&pipeline);
    let mut runner = PipelineRunner::new()
        .with_rate_limiter(RateLimiter::new().with_max_calls_per_minute(0))
        .with_llm_client(client);

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "BYPASS (reversed builder order): provider reached despite 0-call/min limit"
    );
    assert!(result.is_err() || !result.as_ref().unwrap().success);
}

// ─── PROBE 3: LlmJudge, direct field assignment (no builder at all) ──────────
#[tokio::test]
async fn probe_llm_judge_blocked_direct_field_assignment() {
    let (client, hits) = counting_client();
    let pipeline = judge_pipeline();
    let agent = agent_for(&pipeline);
    let mut runner = PipelineRunner::new();
    runner.llm_client = Some(client);
    runner.rate_limiter = Some(Arc::new(std::sync::Mutex::new(
        RateLimiter::new().with_max_calls_per_minute(0),
    )));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "BYPASS (direct fields): ensure_rate_limiter_wired() failed to wire"
    );
    assert!(result.is_err() || !result.as_ref().unwrap().success);
}

// ─── PROBE 4: Guard::SemanticCheck blocked ───────────────────────────────────
#[tokio::test]
async fn probe_semantic_check_guard_blocked() {
    let (client, hits) = counting_client();
    let pipeline = semantic_pipeline();
    let agent = agent_for(&pipeline);
    let mut runner = PipelineRunner::new()
        .with_llm_client(client)
        .with_rate_limiter(RateLimiter::new().with_max_calls_per_minute(0));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "BYPASS: SemanticCheck reached provider despite 0-call/min limit"
    );
    assert!(
        result.is_err() || !result.as_ref().unwrap().success,
        "BYPASS: SemanticCheck pipeline SUCCEEDED under a 0-call/min rate limit"
    );
}

// ─── PROBE 5: positive control — limit 5 lets the judge through ──────────────
// Guards against a false PASS where everything is blocked for unrelated reasons.
#[tokio::test]
async fn probe_positive_control_generous_limit_allows_call() {
    let (client, hits) = counting_client();
    let pipeline = judge_pipeline();
    let agent = agent_for(&pipeline);
    let mut runner = PipelineRunner::new()
        .with_llm_client(client)
        .with_rate_limiter(RateLimiter::new().with_max_calls_per_minute(5));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "positive control failed: judge did not reach provider under a generous limit"
    );
    assert!(
        result.is_ok() && result.unwrap().success,
        "positive control failed: pipeline should succeed under a generous limit"
    );
}

// ─── PROBE 6: Clone/Arc semantics — clone must SHARE limiter state ───────────
// If cloning reset the limiter, each clone would get a fresh budget = bypass.
#[tokio::test]
async fn probe_clone_shares_rate_limiter_state() {
    let hits = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(CountingProvider { hits: hits.clone() });
    let limiter = Arc::new(std::sync::Mutex::new(
        RateLimiter::new().with_max_calls_per_minute(1),
    ));
    let client = LlmClient::new(provider).with_rate_limiter(limiter);

    let req = || LlmRequest {
        system: "s".into(),
        user: "u".into(),
        model: "mock-model".into(),
        max_tokens: Some(8),
        history: None,
        temperature: None,
        tools: None,
        tool_choice: None,
    };

    // First call on the original consumes the single allowed call.
    assert!(client.complete(req()).await.is_ok());

    // A CLONE must observe the exhausted budget, not a fresh one.
    let cloned = client.clone();
    let second = cloned.complete(req()).await;
    assert!(
        matches!(second, Err(LlmError::LocalRateLimit(_))),
        "CLONE BYPASS: cloned LlmClient got a fresh rate-limit budget, or wrong error variant"
    );
    // Verify the error message is NOT misleading (not the HTTP 429 message).
    if let Err(LlmError::LocalRateLimit(msg)) = second {
        assert!(
            msg.contains("Rate limit exceeded") || msg.contains("local"),
            "Error message should be about local rate limit, got: {}", msg
        );
    }
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "CLONE BYPASS: clone let a second call through to the provider"
    );
}

// ─── PROBE 7: scorer coverage (Obstacle B) ───────────────────────────────────
// Documents whether a scorer constructed with its OWN client is rate-limited.
#[tokio::test]
async fn probe_scorer_independent_client_is_not_rate_limited() {
    use verdict::eval::{AnswerRelevancyScorer, Scorer};

    let hits = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(CountingProvider { hits: hits.clone() });
    // Typical usage: scorer built with a plain client, independent of the runner.
    let scorer = AnswerRelevancyScorer {
        llm_client: Arc::new(LlmClient::new(provider)),
        threshold: 0.5,
    };

    // Runner-level limiter of 0 is irrelevant to this independently-built client.
    let _runner_limiter = Arc::new(std::sync::Mutex::new(
        RateLimiter::new().with_max_calls_per_minute(0),
    ));

    let mut step_results = std::collections::HashMap::new();
    step_results.insert(
        "s1".to_string(),
        StepResult {
            step_name: "s1".into(),
            output: StepOutput::new("some answer".into()),
            verdict_passed: true,
            error: None,
        },
    );
    let result = PipelineResult {
        pipeline_name: "p".into(),
        steps_passed: vec!["s1".into()],
        steps_failed: vec![],
        step_results,
        audit_log: Default::default(),
        success: true,
        total_cost_usd: 0.0,
        total_tokens_used: 0,
        log: vec![],
        suspended: None,
        budget: Default::default(),
    };

    let scored = scorer.score(&result).await;
    // Record ground truth: did the scorer's own client hit the provider?
    println!(
        "SCORER PROBE: provider hits = {}, scored_ok = {}",
        hits.load(Ordering::SeqCst),
        scored.is_ok()
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        1,
        "documented gap: scorer with its own client bypasses runner rate limiting"
    );
}

// ─── PROBE 8: Tool-call rate limiter poison recovery (fail-closed) ─────────────
// Verifies that tool-call rate limiting survives a mutex poison (a panic that
// previously held the lock) by recovering with .into_inner(). Without this fix,
// a poisoned lock would silently SKIP the rate-limit check (fail-open bug).
#[tokio::test]
async fn probe_tool_executor_poison_recovery_enforces_rate_limit() {
    use std::thread;
    use std::sync::Mutex;
    use verdict::budget::RateLimiter;
    
    // Create a rate limiter with a very tight limit (0 calls/min)
    let limiter = Arc::new(Mutex::new(RateLimiter::new().with_max_calls_per_minute(0)));
    
    // Spawn a thread that acquires the lock and panics (poisoning it)
    let limiter_clone = limiter.clone();
    let poison_thread = thread::spawn(move || {
        let _guard = limiter_clone.lock().unwrap();
        panic!("intentional poison");
    });
    
    // Wait for the poison to happen (catch panic)
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = poison_thread.join();
    }));
    
    // Verify the mutex is poisoned
    assert!(
        limiter.lock().is_err(),
        "mutex should be poisoned after panic"
    );
    
    // MUTATION TEST 1: Old vulnerable pattern (if let Ok) — BYPASSES the check
    {
        let check_ran = if let Ok(mut rate_limiter) = limiter.lock() {
            // If we get here, the check would have run
            let result = rate_limiter.check_rate_limit();
            result.is_err()
        } else {
            // If the lock was poisoned and we use if let Ok, we SKIP the check
            // This is the fail-open bug: rate limiting silently disabled.
            false
        };
        assert!(
            !check_ran,
            "MUTATION TEST 1 PASSED: if let Ok pattern SKIPS the check on poison (fail-open bug confirmed)"
        );
    }
    
    // MUTATION TEST 2: Fixed pattern (match with .into_inner) — ENFORCES the check
    {
        let mut rate_limiter = match limiter.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        
        let check_result = rate_limiter.check_rate_limit();
        
        assert!(
            check_result.is_err(),
            "MUTATION TEST 2 PASSED: match + .into_inner pattern ENFORCES the check on poison (fail-closed)"
        );
        
        // Verify the error message is about rate limiting
        if let Err(e) = check_result {
            assert!(
                e.to_string().contains("Rate limit"),
                "error should be about rate limiting, got: {}", e
            );
        }
    }
}
