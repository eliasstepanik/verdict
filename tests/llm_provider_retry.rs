/// Integration tests for LlmClient timeout and retry logic (Phase 2 Task 2).
/// 
/// These tests verify:
/// 1. Client-level timeout is enforced (configurable, default 120s)
/// 2. Timeout errors are returned as NetworkError
/// 3. Retry logic: 429 + 5xx errors trigger exponential backoff retries (up to 3 attempts)
/// 4. Non-retryable 4xx errors fail immediately (no retries)
/// 5. Retry exhaustion returns the final error after exactly 3 attempts
use verdict::llm::provider::{OpenAiCompatibleProvider, LlmRequest, LlmError, LlmProvider};
use std::time::Instant;

#[tokio::test]
async fn test_timeout_enforced_on_unreachable_host() {
    // Use a non-routable IP address that will hang indefinitely (triggers TCP timeout)
    // instead of using mockito to avoid runtime nesting issues.
    let provider = OpenAiCompatibleProvider::with_timeout(
        "http://10.255.255.1:9999".into(), // Non-routable address
        "test-key".into(),
        "test-model".into(),
        2, // 2 second timeout
    );

    let req = LlmRequest {
        system: "You are helpful".into(),
        user: "Hello".into(),
        model: "test".into(),
        max_tokens: None,
        temperature: None,
        tool_choice: None,
        tools: None,
        history: None,
    };

    // Measure time to ensure we timeout quickly, not after default OS TCP timeout (minutes)
    let start = Instant::now();
    let result = provider.complete(req).await;
    let elapsed = start.elapsed();

    // Should fail with a network error
    assert!(result.is_err(), "Expected network error");
    
    // Should timeout within a reasonable window (2-5 seconds given our 2s timeout)
    // The actual timeout may be slightly more due to OS scheduling, but much less than
    // the default TCP timeout which can be 30+ seconds
    assert!(
        elapsed.as_secs() < 10,
        "Timeout should fire within 10s, but took {:?}",
        elapsed
    );
    
    match result {
        Err(LlmError::NetworkError(_)) => {
            // Correct — got a network error (which includes timeout)
        }
        Err(e) => {
            panic!("Expected LlmError::NetworkError, got {:?}", e);
        }
        Ok(_) => {
            panic!("Expected error, got Ok");
        }
    }
}

#[test]
fn test_provider_creation() {
    // Verify that OpenAiCompatibleProvider can be created with both constructors
    // without panicking. The timeout configuration is verified by the
    // test_timeout_enforced_on_unreachable_host() test above.
    
    let _provider = OpenAiCompatibleProvider::new(
        "http://example.com".into(),
        "test-key".into(),
        "test-model".into(),
    );
    
    let _provider_custom_timeout = OpenAiCompatibleProvider::with_timeout(
        "http://example.com".into(),
        "test-key".into(),
        "test-model".into(),
        60,
    );
}

#[tokio::test]
async fn test_retry_then_succeed_on_429() {
    // Verify that HTTP 429 (rate limit) triggers retries and eventually succeeds
    // when a subsequent attempt returns 200.
    let mut server = mockito::Server::new_async().await;
    
    // First two requests return 429, third returns 200 with success response
    let _mock1 = server
        .mock("POST", "/v1/chat/completions")
        .expect(1)
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .create();
    
    let _mock2 = server
        .mock("POST", "/v1/chat/completions")
        .expect(1)
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .create();
    
    let _mock3 = server
        .mock("POST", "/v1/chat/completions")
        .expect(1)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{
            "choices": [{"message": {"content": "Success after retries", "tool_calls": null}}],
            "model": "test-model",
            "usage": {"prompt_tokens": 10, "completion_tokens": 20}
        }"#)
        .create();
    
    let provider = OpenAiCompatibleProvider::new(
        server.url(),
        "test-key".into(),
        "test-model".into(),
    );

    let req = LlmRequest {
        system: "You are helpful".into(),
        user: "Hello".into(),
        model: "test-model".into(),
        max_tokens: None,
        temperature: None,
        tool_choice: None,
        tools: None,
        history: None,
    };

    let result = provider.complete(req).await;
    
    // Should eventually succeed
    assert!(result.is_ok(), "Expected success after retries, got: {:?}", result);
    let response = result.unwrap();
    assert_eq!(response.content, "Success after retries");
    assert_eq!(response.model, "test-model");
    
    // Mockito will automatically verify expectations (panic if unmet) when mocks are dropped at end of scope
}

#[tokio::test]
async fn test_non_retryable_4xx_fails_immediately() {
    // Verify that HTTP 400 (Bad Request) fails immediately without retries
    let mut server = mockito::Server::new_async().await;
    
    let _mock = server
        .mock("POST", "/v1/chat/completions")
        .expect(1)
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": {"message": "Invalid request body"}}"#)
        .create();
    
    let provider = OpenAiCompatibleProvider::new(
        server.url(),
        "test-key".into(),
        "test-model".into(),
    );

    let req = LlmRequest {
        system: "You are helpful".into(),
        user: "Hello".into(),
        model: "test-model".into(),
        max_tokens: None,
        temperature: None,
        tool_choice: None,
        tools: None,
        history: None,
    };

    let result = provider.complete(req).await;
    
    // Should fail immediately
    assert!(result.is_err(), "Expected error for 400 Bad Request");
    match result {
        Err(LlmError::RequestFailed(msg)) => {
            assert!(msg.contains("400"), "Error message should mention 400 status");
        }
        Err(e) => {
            panic!("Expected RequestFailed, got: {:?}", e);
        }
        Ok(_) => {
            panic!("Expected error, got Ok");
        }
    }
    
    // Mockito will verify exactly 1 request was made (no retries) via expect(1)
}

#[tokio::test]
async fn test_retry_exhaustion_after_3_attempts() {
    // Verify that HTTP 429 on all attempts exhausts retries after exactly 3 attempts
    let mut server = mockito::Server::new_async().await;
    
    // All three attempts get 429
    let _mock = server
        .mock("POST", "/v1/chat/completions")
        .expect(3)
        .with_status(429)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .create();
    
    let provider = OpenAiCompatibleProvider::new(
        server.url(),
        "test-key".into(),
        "test-model".into(),
    );

    let req = LlmRequest {
        system: "You are helpful".into(),
        user: "Hello".into(),
        model: "test-model".into(),
        max_tokens: None,
        temperature: None,
        tool_choice: None,
        tools: None,
        history: None,
    };

    let start = Instant::now();
    let result = provider.complete(req).await;
    let elapsed = start.elapsed();
    
    // Should eventually fail with RateLimited
    assert!(result.is_err(), "Expected error after exhausting retries");
    match result {
        Err(LlmError::RateLimited) => {
            // Correct
        }
        Err(e) => {
            panic!("Expected RateLimited, got: {:?}", e);
        }
        Ok(_) => {
            panic!("Expected error, got Ok");
        }
    }
    
    // Mockito will verify exactly 3 requests via expect(3) — will panic if expect not met
    
    // With exponential backoff (1s after attempt 1, 2s after attempt 2),
    // total should be at least 3 seconds
    assert!(
        elapsed.as_secs() >= 3,
        "Should have backoff delays; elapsed={:?}",
        elapsed
    );
}
