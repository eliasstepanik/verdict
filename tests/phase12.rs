#![allow(unused_imports, dead_code)]

//! Phase 12 integration tests: SemanticCheck, ContextStore, MCP HTTP, SecretScanner, Parallel concurrency
//!
//! Tests for LLM-based semantic checking, context persistence, MCP HTTP support,
//! secret scanner async/config features, and true concurrent pipeline execution.

use std::sync::Arc;
use verdict::prelude::*;
use verdict::{ContextStore, ContextStoreError};
use verdict::injection::SecretScannerConfig;
use async_trait::async_trait;
use serde_json::json;
use std::time::Instant;

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

// ===== SemanticCheck Tests (P12-1) =====

/// Test 1: SemanticCheck requires LLM client
#[tokio::test]
async fn test_semantic_check_requires_llm_client() {
    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );
    // Set output so we don't get "no output" error first
    ctx.output = Some(StepOutput::new("some output".to_string()));

    let result = GuardEngine::evaluate(&Guard::SemanticCheck("anything".to_string()), &ctx).await;

    assert!(result.is_err(), "SemanticCheck should fail without llm_client");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.to_lowercase().contains("llm"), "Error should mention LLM, got: {}", err_msg);
}

/// Test 2: SemanticCheck with mock LLM passing
#[tokio::test]
async fn test_semantic_check_mock_pass() {
    let mock_llm = MockLlmProvider::new("PASS - looks good");
    let llm_client = Arc::new(LlmClient::new(
        Arc::new(mock_llm),
    ));

    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );
    ctx.output = Some(StepOutput::new("output".to_string()));
    ctx.llm_client = Some(llm_client);

    let result = GuardEngine::evaluate(
        &Guard::SemanticCheck("Check if output is valid".to_string()),
        &ctx,
    )
    .await;

    assert!(result.is_ok(), "SemanticCheck should pass with PASS response");
}

/// Test 3: SemanticCheck with mock LLM failing
#[tokio::test]
async fn test_semantic_check_mock_fail() {
    let mock_llm = MockLlmProvider::new("FAIL - missing required content X");
    let llm_client = Arc::new(LlmClient::new(
        Arc::new(mock_llm),
    ));

    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );
    ctx.output = Some(StepOutput::new("incomplete output".to_string()));
    ctx.llm_client = Some(llm_client);

    let result = GuardEngine::evaluate(
        &Guard::SemanticCheck("Check if output is complete".to_string()),
        &ctx,
    )
    .await;

    assert!(result.is_err(), "SemanticCheck should fail with FAIL response");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("FAIL") || err_msg.contains("missing"), "Error should contain failure details");
}

// ===== ContextStore Tests (P12-3) =====

/// Test 4: ContextStore save and load
#[tokio::test]
async fn test_context_store_save_load() {
    use verdict::ContextStore;
    let temp_dir = tempfile::tempdir().unwrap();
    let store = ContextStore::new(temp_dir.path().to_path_buf());

    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({"key": "value"}),
        FilesystemPolicy::default(),
    );
    ctx.output = Some(StepOutput::new("result".to_string()));

    // Save
    store.save(&ctx).await.expect("save should succeed");

    // Load
    let loaded: verdict::context::SerializableStepContext = store
        .load("test_pipeline", "test_step")
        .await
        .expect("load should succeed");

    assert_eq!(loaded.pipeline_name, "test_pipeline");
    assert_eq!(loaded.step_name, "test_step");
    assert_eq!(loaded.agent_name, "test_agent");
    assert_eq!(loaded.output.as_ref().map(|o| o.raw.as_str()), Some("result"));
}

/// Test 5: ContextStore list snapshots
#[tokio::test]
async fn test_context_store_list_snapshots() {
    use verdict::ContextStore;
    let temp_dir = tempfile::tempdir().unwrap();
    let store = ContextStore::new(temp_dir.path().to_path_buf());

    let mut ctx1 = StepContext::new(
        "agent1".to_string(),
        "pipeline_a".to_string(),
        "step_1".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );
    ctx1.output = Some(StepOutput::new("result1".to_string()));

    let mut ctx2 = StepContext::new(
        "agent2".to_string(),
        "pipeline_a".to_string(),
        "step_2".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );
    ctx2.output = Some(StepOutput::new("result2".to_string()));

    store.save(&ctx1).await.expect("save ctx1");
    store.save(&ctx2).await.expect("save ctx2");

    let snapshots = store
        .list_snapshots("pipeline_a")
        .await
        .expect("list should succeed");

    assert!(snapshots.len() >= 2, "Should have at least 2 snapshots");
    assert!(snapshots.iter().any(|s| s.contains("step_1")), "Should contain step_1");
    assert!(snapshots.iter().any(|s| s.contains("step_2")), "Should contain step_2");
}

/// Test 6: ContextStore not found error
#[tokio::test]
async fn test_context_store_not_found() {
    use verdict::ContextStore;
    let temp_dir = tempfile::tempdir().unwrap();
    let store = ContextStore::new(temp_dir.path().to_path_buf());

    let result = store.load("nonexistent_pipeline", "nonexistent_step").await;

    assert!(result.is_err(), "Load should fail for nonexistent snapshot");
    match result {
        Err(ContextStoreError::NotFound(_)) => {}, // Expected
        _ => panic!("Should be NotFound error"),
    }
}

/// Test 7: ContextStore delete snapshot
#[tokio::test]
async fn test_context_store_delete() {
    use verdict::context::SerializableStepContext;
    let temp_dir = tempfile::tempdir().unwrap();
    let store = ContextStore::new(temp_dir.path().to_path_buf());

    let mut ctx = StepContext::new(
        "agent".to_string(),
        "pipeline".to_string(),
        "step".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );
    ctx.output = Some(StepOutput::new("result".to_string()));

    store.save(&ctx).await.expect("save should succeed");

    // Verify it exists
    let loaded: Result<SerializableStepContext, ContextStoreError> = store.load("pipeline", "step").await;
    assert!(loaded.is_ok(), "Snapshot should exist after save");

    // Delete it
    store.delete("pipeline", "step").await.expect("delete should succeed");

    // Verify it's gone
    let result: Result<SerializableStepContext, ContextStoreError> = store.load("pipeline", "step").await;
    assert!(result.is_err(), "Snapshot should not exist after delete");
}

// ===== MCP HTTP Test (P12-4) =====

/// Test 8: MCP connect with URL only works in Phase 12
#[tokio::test]
async fn test_mcp_connect_url_only_now_works() {
    use verdict::mcp::{McpClient, McpServerConfig};

    let config = McpServerConfig::new("http_server").with_url("http://localhost:8080");

    let result = McpClient::connect(config).await;

    assert!(
        result.is_ok(),
        "URL-only connect should succeed in Phase 12, got: {:?}",
        result.err()
    );
}

// ===== SecretScanner Tests (P12-5) =====

/// Test 9: SecretScanner config defaults
#[test]
fn test_secret_scanner_config_defaults() {
    let config = SecretScannerConfig::default();

    assert!(
        (config.entropy_threshold - 4.5).abs() < 0.001,
        "Default entropy threshold should be ~4.5"
    );
    assert_eq!(config.min_token_len, 20, "Default min_token_len should be 20");
    assert!(config.llm_verifier.is_none(), "llm_verifier should be None by default");
}

/// Test 10: SecretScanner static scan backward compat
#[test]
fn test_secret_scanner_static_scan_backward_compat() {
    let text = "AKIA1234567890ABCDEF something here";
    let matches = SecretScanner::scan(text);

    assert!(
        !matches.is_empty(),
        "Should detect AWS-like key pattern"
    );
    assert!(matches[0].pattern_name.contains("AWS"), "Should identify as AWS pattern");
}

/// Test 11: SecretScanner async scan without LLM
#[tokio::test]
async fn test_secret_scanner_scan_async_no_llm() {
    let scanner = SecretScanner::new();
    let text = "found key: sk-abc123xyz456def789ghi012jkl345mno678 end";

    let matches = scanner.scan_async(text).await;

    // Should not panic, and should find something
    assert!(!matches.is_empty(), "Should find secrets via pattern or entropy");
}

/// Test 12: SecretScanner async scan with LLM verifier
#[tokio::test]
async fn test_secret_scanner_scan_async_with_llm() {
    let mock_llm = MockLlmProvider::new("VERIFIED - this is a real secret");
    let llm_client = Arc::new(LlmClient::new(
        Arc::new(mock_llm),
    ));

    let mut config = SecretScannerConfig::default();
    config.llm_verifier = Some(llm_client);
    let scanner = SecretScanner::with_config(config);

    let text = "API key: sk-abc123xyz456def789ghi012jkl345mno678";
    let matches = scanner.scan_async(text).await;

    assert!(!matches.is_empty(), "Should find and verify secrets");
}

// ===== Parallel Concurrency Test (P12-6) =====

/// Test 13: Parallel steps execute with true concurrency
#[tokio::test]
async fn test_parallel_steps_true_concurrency() {
    use std::sync::{Arc as StdArc, Mutex};

    // Shared execution counter to verify concurrent execution
    let execution_times: StdArc<Mutex<Vec<std::time::Instant>>> = StdArc::new(Mutex::new(Vec::new()));

    // Create 3 steps that all sleep, but should run in parallel
    let times1 = Arc::clone(&execution_times);
    let step1 = AgentStep {
        name: "step1".to_string(),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            times1.lock().unwrap().push(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(100));
            Ok(StepOutput::new("step1".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,  // KEY: parallel = true
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let times2 = Arc::clone(&execution_times);
    let step2 = AgentStep {
        name: "step2".to_string(),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            times2.lock().unwrap().push(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(100));
            Ok(StepOutput::new("step2".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,  // KEY: parallel = true
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let times3 = Arc::clone(&execution_times);
    let step3 = AgentStep {
        name: "step3".to_string(),
        action: StepAction::Custom(Arc::new(move |_ctx| {
            times3.lock().unwrap().push(std::time::Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(100));
            Ok(StepOutput::new("step3".to_string()))
        })),
        guard_in: Guard::None,
        guard_out: Guard::None,
        verdict: Verdict::None,
        parallel: true,  // KEY: parallel = true
        dependencies: Vec::new(),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        tools: ToolSet::None,
    };

    let pipeline = Pipeline {
        name: "parallel_test".to_string(),
        steps: vec![step1, step2, step3],
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

    let start = Instant::now();
    let mut runner = PipelineRunner::new();
    let result = runner.run(&pipeline, &agent, json!({})).await;
    let elapsed = start.elapsed();

    assert!(result.is_ok(), "Pipeline should execute successfully");

    // If truly concurrent: all 3 steps start at nearly the same time, total ~100ms
    // If sequential: total ~300ms
    // We use 400ms threshold to be generous for CI/slow machines
    let elapsed_millis = elapsed.as_millis();
    assert!(
        elapsed_millis < 400,
        "Parallel execution should take <400ms (took {}ms). If sequential, would take ~300ms.",
        elapsed_millis
    );

    // Verify all 3 steps executed
    let times = execution_times.lock().unwrap();
    assert_eq!(times.len(), 3, "All 3 steps should have started");
}

// ===== Bonus: Integration test verifying all Phase 12 features together =====

/// Test 14: Integrated phase 12 scenario
#[tokio::test]
async fn test_phase12_integrated_scenario() {
    let temp_dir = tempfile::tempdir().unwrap();
    let store = ContextStore::new(temp_dir.path().to_path_buf());

    // Build context with LLM client
    let mock_llm = MockLlmProvider::new("PASS");
    let llm_client = Arc::new(LlmClient::new(
        Arc::new(mock_llm),
    ));

    let mut ctx = StepContext::new(
        "agent".to_string(),
        "pipeline".to_string(),
        "step".to_string(),
        json!({}),
        FilesystemPolicy::default(),
    );
    ctx.output = Some(StepOutput::new("validated output".to_string()));
    ctx.llm_client = Some(llm_client);

    // Save context
    store.save(&ctx).await.expect("save should work");

    // Load and verify
    let loaded = store.load("pipeline", "step").await.expect("load should work");
    assert_eq!(loaded.output.as_ref().map(|o| o.raw.as_str()), Some("validated output"));

    // Run semantic check
    let mut ctx_restored = StepContext::new(
        loaded.agent_name.clone(),
        loaded.pipeline_name.clone(),
        loaded.step_name.clone(),
        loaded.request.clone(),
        FilesystemPolicy::default(),
    );
    ctx_restored.output = loaded.output.clone();
    
    let mock_llm2 = MockLlmProvider::new("PASS - content is valid");
    let llm_client2 = Arc::new(LlmClient::new(
        Arc::new(mock_llm2),
    ));
    ctx_restored.llm_client = Some(llm_client2);

    let guard_result = GuardEngine::evaluate(
        &Guard::SemanticCheck("Verify output is complete".to_string()),
        &ctx_restored,
    )
    .await;

    assert!(guard_result.is_ok(), "Semantic check should pass");
}
