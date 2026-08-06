//! Phase 15: Server/Daemon Mode Integration Tests
//!
//! Tests for JSON-RPC server transport and session management over stdio.

use std::sync::Arc;
use tokio::sync::Mutex;
use verdict::prelude::*;

/// Mock in-memory transport for testing (doesn't use real stdio)
struct MockTransport {
    requests: Mutex<Vec<ClientRequest>>,
    responses: Mutex<Vec<ServerEvent>>,
}

impl MockTransport {
    fn new(requests: Vec<ClientRequest>) -> Self {
        MockTransport {
            requests: Mutex::new(requests),
            responses: Mutex::new(vec![]),
        }
    }

    async fn take_responses(&self) -> Vec<ServerEvent> {
        let mut r = self.responses.lock().await;
        let result = r.clone();
        r.clear();
        result
    }
}

#[async_trait::async_trait]
impl ServerTransport for MockTransport {
    async fn next_request(&self) -> Result<Option<ClientRequest>, ServerError> {
        let mut q = self.requests.lock().await;
        if q.is_empty() {
            Ok(None)
        } else {
            Ok(Some(q.remove(0)))
        }
    }

    async fn send_event(&self, event: ServerEvent) -> Result<(), ServerError> {
        self.responses.lock().await.push(event);
        Ok(())
    }

    async fn shutdown(&self) -> Result<(), ServerError> {
        Ok(())
    }
}

/// Helper: create a test echo agent that returns static output
fn create_echo_agent() -> Agent {
    let pipeline = Pipeline {
        name: "echo_pipeline".into(),
        steps: vec![AgentStep {
            name: "echo_step".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|ctx| {
                let input = ctx
                    .input
                    .get("task")
                    .and_then(|v| v.as_str())
                    .unwrap_or("no input");
                Ok(StepOutput::new(format!("Echo: {}", input)))
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
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    Agent {
        name: "echo".into(),
        description: "Echo agent for testing".into(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    }
}

#[tokio::test]
async fn test_server_ping() {
    let runner = Arc::new(Mutex::new(PipelineRunner::new()));
    let session_runner = Arc::new(SessionRunner::new(runner));

    let mock_transport = Arc::new(MockTransport::new(vec![ClientRequest::Ping]));
    let server = AgentServer::new(session_runner, mock_transport.clone());

    // Run the server (will process one Ping request and close)
    let result = server.run().await;
    assert!(result.is_ok());

    // Check we got a Pong response
    let responses = mock_transport.take_responses().await;
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        ServerEvent::Pong => (),
        e => panic!("Expected Pong, got {:?}", e),
    }
}

#[tokio::test]
async fn test_server_new_session() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());
    let runner = PipelineRunner::with_registries(
        Arc::new(ToolRegistry::with_builtins()),
        Arc::new(registry),
    );

    let session_runner = Arc::new(SessionRunner::new(Arc::new(Mutex::new(runner))));

    let mock_transport = Arc::new(MockTransport::new(vec![ClientRequest::NewSession {
        id: "req1".to_string(),
        agent: "echo".to_string(),
        policy: None,
    }]));
    let server = AgentServer::new(session_runner, mock_transport.clone());

    let result = server.run().await;
    assert!(result.is_ok());

    let responses = mock_transport.take_responses().await;
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        ServerEvent::SessionCreated { session_id } => {
            assert!(!session_id.is_empty());
        }
        e => panic!("Expected SessionCreated, got {:?}", e),
    }
}

#[tokio::test]
async fn test_server_list_sessions() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());
    let runner = PipelineRunner::with_registries(
        Arc::new(ToolRegistry::with_builtins()),
        Arc::new(registry),
    );

    let session_runner = Arc::new(SessionRunner::new(Arc::new(Mutex::new(runner))));

    // Create a session first
    let sess_result = session_runner
        .new_session("echo", SessionPolicy::default())
        .await;
    assert!(sess_result.is_ok());

    let mock_transport = Arc::new(MockTransport::new(vec![ClientRequest::ListSessions]));
    let server = AgentServer::new(session_runner, mock_transport.clone());

    let result = server.run().await;
    assert!(result.is_ok());

    let responses = mock_transport.take_responses().await;
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        ServerEvent::SessionList { sessions } => {
            assert_eq!(sessions.len(), 1);
        }
        e => panic!("Expected SessionList, got {:?}", e),
    }
}

#[tokio::test]
async fn test_server_close_session() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());
    let runner = PipelineRunner::with_registries(
        Arc::new(ToolRegistry::with_builtins()),
        Arc::new(registry),
    );

    let session_runner = Arc::new(SessionRunner::new(Arc::new(Mutex::new(runner))));

    // Create a session
    let sess_id = session_runner
        .new_session("echo", SessionPolicy::default())
        .await
        .unwrap();

    let mock_transport = Arc::new(MockTransport::new(vec![ClientRequest::CloseSession {
        session_id: sess_id.to_string(),
    }]));
    let server = AgentServer::new(session_runner, mock_transport.clone());

    let result = server.run().await;
    assert!(result.is_ok());

    let responses = mock_transport.take_responses().await;
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        ServerEvent::SessionClosed { session_id } => {
            assert_eq!(session_id, &sess_id.to_string());
        }
        e => panic!("Expected SessionClosed, got {:?}", e),
    }
}

#[tokio::test]
async fn test_server_policy_reject() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());
    let runner = PipelineRunner::with_registries(
        Arc::new(ToolRegistry::with_builtins()),
        Arc::new(registry),
    );

    let session_runner = Arc::new(SessionRunner::new(Arc::new(Mutex::new(runner))));

    // Create server with restricted agent list
    let mut policy = ServerPolicy::default();
    policy.allowed_agents = vec!["allowed_only".to_string()];

    let mock_transport = Arc::new(MockTransport::new(vec![ClientRequest::NewSession {
        id: "req1".to_string(),
        agent: "echo".to_string(), // Not in allowed list
        policy: None,
    }]));
    let server = AgentServer::new(session_runner, mock_transport.clone()).with_policy(policy);

    let result = server.run().await;
    assert!(result.is_ok());

    let responses = mock_transport.take_responses().await;
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        ServerEvent::Error {
            session_id: Some(id),
            message,
        } => {
            assert_eq!(id, "req1");
            assert!(message.contains("not in allowed list"));
        }
        e => panic!("Expected Error, got {:?}", e),
    }
}

#[tokio::test]
async fn test_server_full_turn() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());
    let runner = PipelineRunner::with_registries(
        Arc::new(ToolRegistry::with_builtins()),
        Arc::new(registry),
    );

    let session_runner = Arc::new(SessionRunner::new(Arc::new(Mutex::new(runner))));

    // Create a session first
    let sess_id = session_runner
        .new_session("echo", SessionPolicy::default())
        .await
        .unwrap();

    let mock_transport = Arc::new(MockTransport::new(vec![ClientRequest::Turn {
        session_id: sess_id.to_string(),
        content: "Hello, Agent!".to_string(),
        attachments: vec![],
    }]));
    let server = AgentServer::new(session_runner, mock_transport.clone());

    let result = server.run().await;
    assert!(result.is_ok());

    let responses = mock_transport.take_responses().await;
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        ServerEvent::TurnCompleted {
            session_id,
            output,
            success,
        } => {
            assert_eq!(session_id, &sess_id.to_string());
            assert!(*success);
            assert!(output.contains("Echo:"));
        }
        e => panic!("Expected TurnCompleted, got {:?}", e),
    }
}

#[tokio::test]
async fn test_server_cancel_turn() {
    let mut registry = AgentRegistry::new();
    registry.register(create_echo_agent());
    let runner = PipelineRunner::with_registries(
        Arc::new(ToolRegistry::with_builtins()),
        Arc::new(registry),
    );

    let session_runner = Arc::new(SessionRunner::new(Arc::new(Mutex::new(runner))));

    // Create a session
    let sess_id = session_runner
        .new_session("echo", SessionPolicy::default())
        .await
        .unwrap();

    let mock_transport = Arc::new(MockTransport::new(vec![ClientRequest::CancelTurn {
        session_id: sess_id.to_string(),
    }]));
    let server = AgentServer::new(session_runner, mock_transport.clone());

    let result = server.run().await;
    assert!(result.is_ok());

    let responses = mock_transport.take_responses().await;
    assert_eq!(responses.len(), 1);
    match &responses[0] {
        ServerEvent::TurnCompleted {
            session_id,
            success,
            ..
        } => {
            assert_eq!(session_id, &sess_id.to_string());
            assert!(!*success); // Cancel is not a successful turn
        }
        e => panic!("Expected TurnCompleted, got {:?}", e),
    }
}
