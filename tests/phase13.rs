//! Phase 13: Interactive Sessions
//!
//! Tests long-lived conversation sessions with persistent state, multi-turn support,
//! and session-scoped guards.

use verdict::prelude::*;
use std::sync::Arc;
use tempfile::TempDir;

/// Test creating a new session
#[tokio::test]
async fn test_session_creation() {
    let mut registry = AgentRegistry::new();

    // Register a simple agent
    let agent = create_echo_agent();
    registry.register(agent.clone());

    let runner = PipelineRunner::with_agent_registry(Arc::new(registry));
    let session_runner = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner)));

    let policy = SessionPolicy::default();
    let session_id = session_runner
        .new_session("echo_agent", policy)
        .await
        .expect("should create session");

    assert!(!session_id.as_str().is_empty());
}

/// Test session turn execution and conversation history
#[tokio::test]
async fn test_session_turn_execution() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());

    let runner = PipelineRunner::with_agent_registry(Arc::new(registry));
    let session_runner = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner)));

    let policy = SessionPolicy::default();
    let session_id = session_runner
        .new_session("echo_agent", policy)
        .await
        .expect("should create session");

    // Execute a turn
    let turn = UserTurn::text("Hello, agent!");
    let result = session_runner
        .turn(&session_id, turn)
        .await
        .expect("should execute turn");

    match result {
        TurnResult::Completed { output, usage } => {
            assert!(output.contains("Echo:"));
            let _ = usage.total_tokens; // u32, always valid
        }
        _ => panic!("expected Completed turn result"),
    }
}

/// Test multiple turns in a session
#[tokio::test]
async fn test_session_multiple_turns() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());

    let runner = PipelineRunner::with_agent_registry(Arc::new(registry));
    let session_runner = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner)));

    let policy = SessionPolicy::default();
    let session_id = session_runner
        .new_session("echo_agent", policy)
        .await
        .expect("should create session");

    // Execute 10 turns
    for i in 0..10 {
        let turn = UserTurn::text(format!("Message {}", i));
        let _result = session_runner
            .turn(&session_id, turn)
            .await
            .expect("should execute turn");
    }

    // Check session metadata
    let meta = session_runner
        .get_meta(&session_id)
        .await
        .expect("should get metadata");

    assert_eq!(meta.turn_count, 10, "should have 10 turns");
}

/// Test session persistence
#[tokio::test]
async fn test_session_persistence() {
    let temp_dir = TempDir::new().expect("should create temp dir");
    let persist_path = temp_dir.path().to_path_buf();

    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());

    let runner = PipelineRunner::with_agent_registry(Arc::new(registry));
    let session_runner = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner)))
        .with_persistence(persist_path.clone());

    let policy = SessionPolicy::default();
    let session_id = session_runner
        .new_session("echo_agent", policy)
        .await
        .expect("should create session");

    // Execute 10 turns
    for i in 0..10 {
        let turn = UserTurn::text(format!("Message {}", i));
        let _result = session_runner
            .turn(&session_id, turn)
            .await
            .expect("should execute turn");
    }

    // Create a new runner pointing to the same persist dir
    let mut registry2 = AgentRegistry::new();
    registry2.register(create_echo_agent());
    let runner2 = PipelineRunner::with_agent_registry(Arc::new(registry2));
    let session_runner2 = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner2)))
        .with_persistence(persist_path);

    // Resume the session
    let meta = session_runner2
        .resume(&session_id)
        .await
        .expect("should resume session");

    assert_eq!(meta.turn_count, 10, "should have persisted 10 turns");

    // Send one more turn
    let turn = UserTurn::text("Final message");
    let result = session_runner2
        .turn(&session_id, turn)
        .await
        .expect("should execute turn");

    match result {
        TurnResult::Completed { .. } => {
            // Check the turn was recorded
            let updated_meta = session_runner2
                .get_meta(&session_id)
                .await
                .expect("should get metadata");
            assert_eq!(updated_meta.turn_count, 11, "should have 11 turns after resume");
        }
        _ => panic!("expected Completed turn result"),
    }
}

/// Test session turn limit
#[tokio::test]
async fn test_session_turn_limit_guard() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());

    let runner = PipelineRunner::with_agent_registry(Arc::new(registry));
    let session_runner = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner)));

    let policy = SessionPolicy {
        max_turns: Some(3),
        ..Default::default()
    };
    let session_id = session_runner
        .new_session("echo_agent", policy)
        .await
        .expect("should create session");

    // Execute 3 turns (at the limit)
    for i in 0..3 {
        let turn = UserTurn::text(format!("Message {}", i));
        let _result = session_runner
            .turn(&session_id, turn)
            .await
            .expect("should execute turn");
    }

    // 4th turn should fail
    let turn = UserTurn::text("Message 3");
    let result = session_runner.turn(&session_id, turn).await;

    match result {
        Err(SessionError::LimitExceeded(_)) => {
            // Expected
        }
        _ => panic!("expected LimitExceeded error"),
    }
}

/// Test session close
#[tokio::test]
async fn test_session_close() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());

    let runner = PipelineRunner::with_agent_registry(Arc::new(registry));
    let session_runner = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner)));

    let policy = SessionPolicy::default();
    let session_id = session_runner
        .new_session("echo_agent", policy)
        .await
        .expect("should create session");

    // Close the session
    let meta = session_runner
        .close(&session_id)
        .await
        .expect("should close session");

    assert_eq!(meta.id, session_id);

    // Try to execute a turn on closed session (should fail)
    let turn = UserTurn::text("Message");
    let result = session_runner.turn(&session_id, turn).await;

    match result {
        Err(SessionError::Closed) => {
            // Expected
        }
        _ => panic!("expected Closed error"),
    }
}

/// Test conversation history
#[tokio::test]
async fn test_conversation_history() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());

    let runner = PipelineRunner::with_agent_registry(Arc::new(registry));
    let session_runner = SessionRunner::new(Arc::new(tokio::sync::Mutex::new(runner)));

    let policy = SessionPolicy::default();
    let session_id = session_runner
        .new_session("echo_agent", policy)
        .await
        .expect("should create session");

    // Execute some turns
    session_runner
        .turn(&session_id, UserTurn::text("Hello"))
        .await
        .expect("should execute turn");

    session_runner
        .turn(&session_id, UserTurn::text("World"))
        .await
        .expect("should execute turn");

    // Check metadata
    let meta = session_runner
        .get_meta(&session_id)
        .await
        .expect("should get metadata");

    assert_eq!(meta.turn_count, 2);
    assert!(meta.total_tokens.total_tokens > 0);
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Create a simple echo agent for testing
fn create_echo_agent() -> Agent {
    use verdict::prelude::*;
    use std::sync::Arc;

    let echo_step = AgentStep {
        name: "echo".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|ctx| {
            let input = ctx.input["task"]
                .as_str()
                .unwrap_or("(empty)")
                .to_string();
            let output = format!("Echo: {}", input);
            Ok(StepOutput::new(output))
        })),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        };

    Agent {
        name: "echo_agent".into(),
        description: "Simple echo agent for testing".into(),
        pipeline: Pipeline {
            name: "echo_pipeline".into(),
            steps: vec![echo_step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    }
}

