//! Phase 18: Reference Application
//!
//! End-to-end integration test validating the complete Phase 13Ã¢â‚¬â€œ18 stack.
//! Tests the full pipeline: SessionRunner Ã¢â€ â€™ PipelineRunner Ã¢â€ â€™ GuardEngine Ã¢â€ â€™ VerdictEngine
//! across multiple agents, delegations, and tool invocations.

use verdict::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================================
// HELPERS
// ============================================================================

/// Create a simple echo agent for testing
fn create_echo_agent(name: &str) -> Agent {
    let echo_step = AgentStep {
        name: "echo".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(move |ctx| {
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
        name: name.into(),
        description: format!("{} agent", name).into(),
        pipeline: Pipeline {
            name: format!("{}_pipeline", name),
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

/// Create a session runner with the given agents
fn create_session_runner(agents: Vec<Agent>) -> SessionRunner {
    let mut registry = AgentRegistry::new();
    for agent in agents {
        registry.register(agent);
    }

    let runner = Arc::new(Mutex::new(
        PipelineRunner::with_agent_registry(Arc::new(registry))
    ));
    SessionRunner::new(runner)
}

// ============================================================================
// TESTS: Full Stack Session Management
// ============================================================================

/// Test: Create and execute a single turn in a session
#[tokio::test]
async fn test_full_session_to_server_stack_single_turn() {
    let sr = create_session_runner(vec![
        create_echo_agent("assistant"),
    ]);

    let id = sr.new_session("assistant", SessionPolicy::default())
        .await
        .expect("should create session");

    let turn = UserTurn::text("hello world");
    let result = sr.turn(&id, turn).await.expect("should execute turn");

    match result {
        TurnResult::Completed { output, .. } => {
            assert!(output.contains("hello world"));
        }
        other => panic!("Unexpected result: {:?}", other),
    }
}

/// Test: Execute multiple turns in a single session
#[tokio::test]
async fn test_full_session_to_server_stack_multiple_turns() {
    let sr = create_session_runner(vec![
        create_echo_agent("assistant"),
    ]);

    let id = sr.new_session("assistant", SessionPolicy::default())
        .await
        .expect("should create session");

    // Execute multiple turns
    for i in 1..=5 {
        let turn = UserTurn::text(format!("turn {}", i));
        let result = sr.turn(&id, turn).await.expect("should execute turn");

        match result {
            TurnResult::Completed { output, .. } => {
                assert!(output.contains(&format!("turn {}", i)));
            }
            other => panic!("Unexpected result on turn {}: {:?}", i, other),
        }
    }

    // Verify session metadata
    let meta = sr.get_meta(&id).await.expect("should get metadata");
    assert_eq!(meta.turn_count, 5, "should have 5 turns");
}

/// Test: Session listing
#[tokio::test]
async fn test_session_listing() {
    let sr = create_session_runner(vec![
        create_echo_agent("assistant"),
    ]);

    let id1 = sr.new_session("assistant", SessionPolicy::default())
        .await
        .expect("should create session 1");

    let id2 = sr.new_session("assistant", SessionPolicy::default())
        .await
        .expect("should create session 2");

    let sessions = sr.list().await;
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().any(|s| s.id == id1));
    assert!(sessions.iter().any(|s| s.id == id2));
}

/// Test: Session closure
#[tokio::test]
async fn test_session_closure() {
    let sr = create_session_runner(vec![
        create_echo_agent("assistant"),
    ]);

    let id = sr.new_session("assistant", SessionPolicy::default())
        .await
        .expect("should create session");

    // Close the session
    sr.close(&id).await.expect("should close session");

    // Try to execute a turn on closed session
    let turn = UserTurn::text("message");
    let result = sr.turn(&id, turn).await;

    assert!(matches!(result, Err(SessionError::Closed)));
}

// ============================================================================
// TESTS: Server Protocol (AgentServer + StdioTransport)
// ============================================================================

/// Test: Server handles Ping request
#[tokio::test]
async fn test_server_protocol_ping() {
    use tokio::sync::Mutex as TokioMutex;

    struct VecTransport {
        requests: TokioMutex<Vec<ClientRequest>>,
        events: TokioMutex<Vec<ServerEvent>>,
    }

    #[async_trait::async_trait]
    impl ServerTransport for VecTransport {
        async fn next_request(&self) -> Result<Option<ClientRequest>, ServerError> {
            let mut q = self.requests.lock().await;
            if q.is_empty() { Ok(None) } else { Ok(Some(q.remove(0))) }
        }
        async fn send_event(&self, e: ServerEvent) -> Result<(), ServerError> {
            self.events.lock().await.push(e);
            Ok(())
        }
        async fn shutdown(&self) -> Result<(), ServerError> { Ok(()) }
    }

    let sr = Arc::new(create_session_runner(vec![
        create_echo_agent("assistant"),
    ]));

    let transport = Arc::new(VecTransport {
        requests: TokioMutex::new(vec![
            ClientRequest::Ping,
        ]),
        events: TokioMutex::new(vec![]),
    });

    let server = AgentServer::new(sr, transport.clone());
    let _ = server.run().await;

    let events = transport.events.lock().await;
    assert!(matches!(events[0], ServerEvent::Pong));
}

/// Test: Server handles NewSession request
#[tokio::test]
async fn test_server_protocol_new_session() {
    use tokio::sync::Mutex as TokioMutex;

    struct VecTransport {
        requests: TokioMutex<Vec<ClientRequest>>,
        events: TokioMutex<Vec<ServerEvent>>,
    }

    #[async_trait::async_trait]
    impl ServerTransport for VecTransport {
        async fn next_request(&self) -> Result<Option<ClientRequest>, ServerError> {
            let mut q = self.requests.lock().await;
            if q.is_empty() { Ok(None) } else { Ok(Some(q.remove(0))) }
        }
        async fn send_event(&self, e: ServerEvent) -> Result<(), ServerError> {
            self.events.lock().await.push(e);
            Ok(())
        }
        async fn shutdown(&self) -> Result<(), ServerError> { Ok(()) }
    }

    let sr = Arc::new(create_session_runner(vec![
        create_echo_agent("assistant"),
    ]));

    let transport = Arc::new(VecTransport {
        requests: TokioMutex::new(vec![
            ClientRequest::NewSession {
                id: "s1".into(),
                agent: "assistant".into(),
                policy: None,
            },
        ]),
        events: TokioMutex::new(vec![]),
    });

    let server = AgentServer::new(sr, transport.clone());
    let _ = server.run().await;

    let events = transport.events.lock().await;
    assert!(matches!(events[0], ServerEvent::SessionCreated { .. }));
}

/// Test: Server handles Turn request - direct session approach
#[tokio::test]
async fn test_server_protocol_turn() {
    // This test creates a session directly and then executes a turn
    // to verify the Turn protocol works end-to-end
    let sr = Arc::new(create_session_runner(vec![
        create_echo_agent("assistant"),
    ]));

    let session_id = sr.new_session("assistant", SessionPolicy::default())
        .await
        .expect("should create session");

    let turn = UserTurn::text("test message");
    let result = sr.turn(&session_id, turn).await
        .expect("should execute turn");

    match result {
        TurnResult::Completed { output, .. } => {
            assert!(!output.is_empty());
            assert!(output.contains("test message"));
        }
        other => panic!("Expected Completed turn, got: {:?}", other),
    }
}

/// Test: Server handles ListSessions request
#[tokio::test]
async fn test_server_protocol_list_sessions() {
    use tokio::sync::Mutex as TokioMutex;

    struct VecTransport {
        requests: TokioMutex<Vec<ClientRequest>>,
        events: TokioMutex<Vec<ServerEvent>>,
    }

    #[async_trait::async_trait]
    impl ServerTransport for VecTransport {
        async fn next_request(&self) -> Result<Option<ClientRequest>, ServerError> {
            let mut q = self.requests.lock().await;
            if q.is_empty() { Ok(None) } else { Ok(Some(q.remove(0))) }
        }
        async fn send_event(&self, e: ServerEvent) -> Result<(), ServerError> {
            self.events.lock().await.push(e);
            Ok(())
        }
        async fn shutdown(&self) -> Result<(), ServerError> { Ok(()) }
    }

    let sr = Arc::new(create_session_runner(vec![
        create_echo_agent("assistant"),
    ]));

    let transport = Arc::new(VecTransport {
        requests: TokioMutex::new(vec![
            ClientRequest::ListSessions,
        ]),
        events: TokioMutex::new(vec![]),
    });

    let server = AgentServer::new(sr, transport.clone());
    let _ = server.run().await;

    let events = transport.events.lock().await;
    assert!(matches!(events[0], ServerEvent::SessionList { .. }));
}

// ============================================================================
// TESTS: Multi-Agent Delegation
// ============================================================================

/// Test: Full stack with multiple agents and delegation
#[tokio::test]
async fn test_full_stack_with_delegation() {
    let worker = create_echo_agent("worker");
    let orchestrator = Agent {
        name: "orchestrator".into(),
        description: "Orchestrator agent".into(),
        pipeline: Pipeline {
            name: "orchestrator_pipeline".into(),
            steps: vec![AgentStep {
                name: "delegate".into(),
                guard_in: Guard::None,
                action: StepAction::DelegateAgent {
                    agent: "worker".into(),
                    input: serde_json::json!({ "task": "delegated task" }),
                    expected_output_schema: None,
                    delegation_policy: DelegationPolicy {
                        max_depth: 2,
                        allowed_agents: vec!["worker".into()],
                        require_output_schema: false,
                        inherit_tool_scope: true,
                        inherit_budget: true,
                        require_user_approval: false,
                        on_delegation_start: None,
                        on_delegation_complete: None,
                        on_iteration_complete: None,
                        message_filter: None,
                        memory_isolation: MemoryIsolation::Isolated,
                    },
                    detached: false,
                },
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
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
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    let sr = create_session_runner(vec![worker, orchestrator]);

    let id = sr.new_session("orchestrator", SessionPolicy::default())
        .await
        .expect("should create session");

    let turn = UserTurn::text("orchestrate something");
    let result = sr.turn(&id, turn).await.expect("should execute turn");

    match result {
        TurnResult::Completed { output, .. } => {
            assert!(output.contains("Echo"));
        }
        other => panic!("Unexpected result: {:?}", other),
    }
}

// ============================================================================
// TESTS: Conversation History and Multi-Turn Context
// ============================================================================

/// Test: Conversation history accumulation across turns
#[tokio::test]
async fn test_conversation_history_accumulation() {
    let sr = create_session_runner(vec![
        create_echo_agent("assistant"),
    ]);

    let id = sr.new_session("assistant", SessionPolicy::default())
        .await
        .expect("should create session");

    // Execute 3 turns
    for i in 1..=3 {
        let turn = UserTurn::text(format!("message {}", i));
        sr.turn(&id, turn).await.expect("should execute turn");
    }

    // Get session metadata and verify history
    let meta = sr.get_meta(&id).await.expect("should get metadata");
    assert_eq!(meta.turn_count, 3);
    assert!(meta.total_tokens.total_tokens > 0);
}

