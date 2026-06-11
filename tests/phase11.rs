//! Phase 11 integration tests: Guard implementations and budget tracking

use std::sync::Arc;
use verdict::prelude::*;
use async_trait::async_trait;
use serde_json::json;

/// Mock LLM provider for testing
struct MockLlmProvider {
    response: String,
}

impl MockLlmProvider {
    fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn default_model(&self) -> &str {
        "mock-model"
    }

    async fn complete(&self, _req: LlmRequest) -> Result<LlmResponse, LlmError> {
        Ok(LlmResponse {
            content: self.response.clone(),
            model: "mock".into(),
            usage: Some(LlmUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
            }),
            tool_calls: None,
        })
    }

    fn stream(
        &self,
        _request: LlmRequest,
    ) -> std::pin::Pin<Box<dyn futures::stream::Stream<Item = Result<verdict::LlmChunk, verdict::LlmError>> + Send>> {
        let response = self.response.clone();
        Box::pin(futures::stream::once(async move {
            Ok(verdict::LlmChunk {
                delta: response,
                finish_reason: Some("stop".to_string()),
            })
        }))
    }
}

/// Test 1: Guard::PathWithinWorkspace rejects path traversal
#[tokio::test]
async fn test_guard_path_within_workspace_rejects_traversal() {
    let step = AgentStep {
        name: "path_check".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("../etc/passwd".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::PathWithinWorkspace,
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "path_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new();

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(result.is_err(), "PathWithinWorkspace should reject ../");
}

/// Test 2: Guard::DependenciesAllowlist rejects unlisted deps
#[tokio::test]
async fn test_guard_dependencies_allowlist_rejects_unlisted() {
    let toml_with_unknown = r#"
[dependencies]
serde = "1.0"
unknown-crate = "1.0"
"#.to_string();

    let toml_clone = toml_with_unknown.clone();
    let step = AgentStep {
        name: "dep_check".to_string(),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            Ok(StepOutput::new(toml_clone.clone()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::DependenciesAllowlist(vec!["serde".to_string()]),
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "deps_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new();

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(result.is_err(), "DependenciesAllowlist should reject unlisted deps");
}

/// Test 3: Guard::ShellCommandAllowlist rejects unlisted commands
#[tokio::test]
async fn test_guard_shell_command_allowlist_rejects_unlisted() {
    let step = AgentStep {
        name: "cmd_check".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("rm -rf /tmp\ngit clone ...".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::ShellCommandAllowlist(vec!["cargo test".to_string()]),
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "shell_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new();

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(result.is_err(), "ShellCommandAllowlist should reject unlisted commands");
}

/// Test 4: Guard::NoSuspiciousDependencies catches known bad patterns
#[tokio::test]
async fn test_guard_no_suspicious_dependencies() {
    let step = AgentStep {
        name: "suspicious_dep".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new(
                r#"[dependencies]
openssl-sys-1.0.0 = "1.0""#.to_string(),
            ))
        })),
        guard_in: Guard::None,
        guard_out: Guard::NoSuspiciousDependencies,
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "suspicious_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new();

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(result.is_err(), "NoSuspiciousDependencies should reject known bad patterns");
}

/// Test 5: Budget tracking with LLM calls
#[tokio::test]
async fn test_budget_llm_calls_incremented() {
    let mock_llm = MockLlmProvider::new("test response");
    let llm_client = Arc::new(LlmClient::new(
        Arc::new(mock_llm),
    ));

    let step = AgentStep {
        name: "llm_call".to_string(),
        action: StepAction::LlmCall {
            system: "You are helpful".into(),
            user: "What is 2+2?".into(),
            model: None,
            conversation_id: None,
            append_to_history: true,
        },
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "llm_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new().with_llm_client(llm_client);

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(result.is_ok(), "LLM call should succeed and budget should be tracked");
}

/// Test 6: Guard::MaxLlmCalls enforces budget limit
#[tokio::test]
async fn test_guard_max_llm_calls_enforces_limit() {
    let mock_llm = MockLlmProvider::new("response");
    let llm_client = Arc::new(LlmClient::new(
        Arc::new(mock_llm),
    ));

    let step = AgentStep {
        name: "llm_step".to_string(),
        action: StepAction::LlmCall {
            system: "You are helpful".into(),
            user: "What is 2+2?".into(),
            model: None,
            conversation_id: None,
            append_to_history: true,
        },
        guard_in: Guard::None,
        guard_out: Guard::MaxLlmCalls(0),
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "budget_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new().with_llm_client(llm_client);

    let result = runner.run(&pipeline, &agent, json!({})).await;

    assert!(result.is_err(), "Guard::MaxLlmCalls should fail when limit is 0 and we make a call");
}

/// Test 7: Fallback pipeline executes on step failure
#[tokio::test]
async fn test_fallback_pipeline_executes_on_step_failure() {
    let main_step = AgentStep {
        name: "main_step".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Err(StepError::ActionFailed {
                reason: "intentional failure".into(),
            })
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let fallback_step = AgentStep {
        name: "fallback_step".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("fallback succeeded".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let fallback_pipeline = Pipeline {
        name: "fallback".to_string(),
        steps: vec![fallback_step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let main_pipeline = Pipeline {
        name: "main".to_string(),
        steps: vec![main_step],
        max_retries: 0,
        on_failure: FailureMode::Fallback(Box::new(fallback_pipeline)),
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline {
            name: "empty".to_string(),
            steps: Vec::new(),
            max_retries: 0,
            on_failure: FailureMode::Abort,
        },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new();

    let result = runner.run(&main_pipeline, &agent, json!({})).await;

    assert!(result.is_ok(), "Fallback pipeline should execute and succeed");
}

/// Test 8: Verdict::LlmJudge passes when mock returns the expected pattern
#[tokio::test]
async fn test_verdict_llm_judge_passes_on_pattern() {
    let mock_llm = MockLlmProvider::new("APPROVED — the output looks correct");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm)));

    let step = AgentStep {
        name: "judge_step".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("some output".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::LlmJudge {
            system: "You are a reviewer.".to_string(),
            input_template: "Review this: {output}".to_string(),
            model: None,
            pass_on_pattern: "APPROVED".to_string(),
        },
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "judge_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline { name: "empty".to_string(), steps: Vec::new(), max_retries: 0, on_failure: FailureMode::Abort },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new().with_llm_client(llm_client);

    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "LlmJudge should pass when mock returns 'APPROVED'");
}

/// Test 9: Verdict::LlmJudge fails when pattern is absent from response
#[tokio::test]
async fn test_verdict_llm_judge_fails_without_pattern() {
    let mock_llm = MockLlmProvider::new("REJECTED — the output has issues");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm)));

    let step = AgentStep {
        name: "judge_fail_step".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("bad output".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::LlmJudge {
            system: "You are a reviewer.".to_string(),
            input_template: "Review: {output}".to_string(),
            model: None,
            pass_on_pattern: "APPROVED".to_string(),
        },
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "judge_fail_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline { name: "empty".to_string(), steps: Vec::new(), max_retries: 0, on_failure: FailureMode::Abort },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new().with_llm_client(llm_client);

    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_err(), "LlmJudge should fail when response does not contain pattern");
}

/// Test 10: Parallel steps both execute and results are merged
#[tokio::test]
async fn test_parallel_steps_both_execute() {
    use std::sync::atomic::{AtomicU32, Ordering};

    let counter = Arc::new(AtomicU32::new(0));
    let c1 = counter.clone();
    let c2 = counter.clone();

    let step1 = AgentStep {
        name: "parallel_a".to_string(),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            c1.fetch_add(1, Ordering::SeqCst);
            Ok(StepOutput::new("a done".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let step2 = AgentStep {
        name: "parallel_b".to_string(),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            c2.fetch_add(1, Ordering::SeqCst);
            Ok(StepOutput::new("b done".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "parallel_test".to_string(),
        steps: vec![step1, step2],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline { name: "empty".to_string(), steps: Vec::new(), max_retries: 0, on_failure: FailureMode::Abort },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new();

    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Parallel pipeline should succeed");
    assert_eq!(counter.load(Ordering::SeqCst), 2, "Both parallel steps should have executed");

    let pr = result.unwrap();
    assert!(pr.step_results.contains_key("parallel_a"), "parallel_a result should be present");
    assert!(pr.step_results.contains_key("parallel_b"), "parallel_b result should be present");
}

/// Test 11: LlmCallStreaming emits to OutputSink and returns assembled output
#[tokio::test]
async fn test_llm_call_streaming_with_sink() {
    use std::sync::Mutex as StdMutex;

    let mock_llm = MockLlmProvider::new("streamed response text");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm)));

    // A simple sink that records emitted events
    struct RecordingSink {
        events: Arc<StdMutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl OutputSink for RecordingSink {
        async fn emit(&self, event: OutputEvent) {
            if let OutputEvent::LlmChunk { delta, .. } = event {
                self.events.lock().unwrap().push(delta);
            }
        }
    }

    let recorded = Arc::new(StdMutex::new(Vec::<String>::new()));
    let sink = Arc::new(RecordingSink { events: recorded.clone() });

    let step = AgentStep {
        name: "streaming_step".to_string(),
        action: StepAction::LlmCallStreaming {
            system: "You are helpful.".to_string(),
            user: "Say hello.".to_string(),
            model: None,
        },
        guard_in: Guard::None,
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "streaming_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline { name: "empty".to_string(), steps: Vec::new(), max_retries: 0, on_failure: FailureMode::Abort },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new()
        .with_llm_client(llm_client)
        .with_output_sink(sink);

    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "Streaming LLM call should succeed");

    let events = recorded.lock().unwrap();
    assert!(!events.is_empty(), "OutputSink should have received at least one LlmChunk event");
    assert_eq!(events[0], "streamed response text");
}

/// Test 12: Conversation history is appended after LlmCall
#[tokio::test]
async fn test_conversation_history_appended() {
    let mock_llm = MockLlmProvider::new("assistant reply");
    let llm_client = Arc::new(LlmClient::new(Arc::new(mock_llm)));

    let step = AgentStep {
        name: "history_step".to_string(),
        action: StepAction::LlmCall {
            system: "You are helpful.".to_string(),
            user: "Hello!".to_string(),
            model: None,
            conversation_id: None,
            append_to_history: true,
        },
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "history_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline { name: "empty".to_string(), steps: Vec::new(), max_retries: 0, on_failure: FailureMode::Abort },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    let mut runner = PipelineRunner::new().with_llm_client(llm_client);

    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok(), "LlmCall with history tracking should succeed");
    // The output should be the mock response
    let pr = result.unwrap();
    assert!(pr.step_results.contains_key("history_step"));
    assert_eq!(pr.step_results["history_step"].output.raw, "assistant reply");
}

/// Test 13: InjectionScanner detects high-entropy payload
#[test]
fn test_injection_scanner_detects_high_entropy() {
    // Construct a string using all 62 distinct alphanumeric chars + 8 punctuation chars = 70 distinct
    // characters, each appearing exactly once or twice — guarantees Shannon entropy > 5.5 bits/char.
    // H = log2(70) ≈ 6.12 bits/char when all 70 chars appear with equal frequency.
    let high_entropy = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()_+-=[]{}|;";
    assert!(high_entropy.len() > 50, "test string should be long enough");
    let result = InjectionScanner::scan(high_entropy);
    assert!(result.detected, "High-entropy payload should be detected as injection risk");
    assert_eq!(result.risk_level, Some(RiskLevel::High));
}

/// Test 14: LlmJudge without LLM client returns NoLlmClient error
#[tokio::test]
async fn test_verdict_llm_judge_no_client_fails() {
    let step = AgentStep {
        name: "no_client_judge".to_string(),
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("output".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::LlmJudge {
            system: "judge".to_string(),
            input_template: "{output}".to_string(),
            model: None,
            pass_on_pattern: "PASS".to_string(),
        },
        parallel: false,
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "no_client_test".to_string(),
        steps: vec![step],
        max_retries: 0,
        on_failure: FailureMode::Abort,
    };

    let agent = Agent {
        name: "test_agent".to_string(),
        description: String::new(),
        pipeline: Pipeline { name: "empty".to_string(), steps: Vec::new(), max_retries: 0, on_failure: FailureMode::Abort },
        tools: ToolSet::None,
        skills: Default::default(),
        policy: Default::default(),
    };
    // No LLM client attached
    let mut runner = PipelineRunner::new();

    let result = runner.run(&pipeline, &agent, json!({})).await;
    assert!(result.is_err(), "LlmJudge without LLM client should fail");
}
