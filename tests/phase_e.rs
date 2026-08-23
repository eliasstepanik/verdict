use std::sync::Arc;
use verdict::prelude::*;

// Phase E: Observability & Deployment Integration Tests
// This validates the monitoring server enhancements and registry extensions

#[test]
fn test_monitoring_server_with_agent_registry() {
    use verdict::audit::{AuditLog, MonitoringServer};
    use verdict::context::PipelineTrace;
    use verdict::registry::AgentRegistry;

    let audit_log = AuditLog::new();
    let trace = PipelineTrace::new();
    let agent_registry = Arc::new(AgentRegistry::new());

    let _server = MonitoringServer::new(audit_log, trace).with_agent_registry(agent_registry);

    // Verify we can construct monitoring server with agent registry
    assert!(true);
}

#[test]
fn test_monitoring_server_with_conversation_registry() {
    use std::sync::Mutex;
    use verdict::audit::{AuditLog, MonitoringServer};
    use verdict::context::PipelineTrace;
    use verdict::llm::ConversationRegistry;

    let audit_log = AuditLog::new();
    let trace = PipelineTrace::new();
    let conversation_registry = Arc::new(Mutex::new(ConversationRegistry::new()));

    let _server =
        MonitoringServer::new(audit_log, trace).with_conversation_registry(conversation_registry);

    // Verify we can construct monitoring server with conversation registry
    assert!(true);
}

#[test]
fn test_agent_registry_list_agents() {
    use verdict::agent::Agent;
    use verdict::registry::AgentRegistry;

    let mut registry = AgentRegistry::new();

    let agent = Agent {
        name: "test_agent".to_string(),
        description: "A test agent".to_string(),
        pipeline: Pipeline {
            name: "test_pipeline".to_string(),
            steps: vec![],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::ReadOnly,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    registry.register(agent);
    let agents = registry.list_agents();

    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].name, "test_agent");
}

#[test]
fn test_conversation_registry_list_conversations() {
    use verdict::llm::ConversationRegistry;

    let mut registry = ConversationRegistry::new();
    registry.get_or_create("conv-1");

    let conversations = registry.list_conversations();
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].0, "conv-1");
}

#[test]
fn test_pipeline_trace_creation() {
    use verdict::context::PipelineTrace;

    let trace = PipelineTrace::new();
    assert_eq!(trace.entries.len(), 0);
}

#[test]
fn test_agent_registry_multiple_agents() {
    use verdict::agent::Agent;
    use verdict::registry::AgentRegistry;

    let mut registry = AgentRegistry::new();

    for i in 0..3 {
        let agent = Agent {
            name: format!("agent{}", i),
            description: format!("Test agent {}", i),
            pipeline: Pipeline {
                name: format!("pipeline{}", i),
                steps: vec![],
                on_failure: FailureMode::Abort,
                max_retries: 0,
            },
            tools: ToolSet::ReadOnly,
            skills: SkillSet { skills: vec![] },
            policy: AgentPolicy::default(),
            scorers: vec![],
        };
        registry.register(agent);
    }

    let agents = registry.list_agents();
    assert_eq!(agents.len(), 3);
}

#[test]
fn test_conversation_registry_multiple_conversations() {
    use verdict::llm::ConversationRegistry;

    let mut registry = ConversationRegistry::new();

    for i in 0..5 {
        registry.get_or_create(&format!("conv-{}", i));
    }

    let conversations = registry.list_conversations();
    assert_eq!(conversations.len(), 5);
}

#[test]
fn test_monitoring_server_construction() {
    use verdict::audit::{AuditLog, MonitoringServer};
    use verdict::context::PipelineTrace;

    let audit_log = AuditLog::new();
    let trace = PipelineTrace::new();

    let _server = MonitoringServer::new(audit_log, trace);

    // Verify basic construction works
    assert!(true);
}

/// Behavioral test: TimeoutLayer actually enforces timeouts
/// Uses a real server on a real port with a delayed handler to prove timeout fires
#[tokio::test]
async fn test_monitoring_server_timeout_layer_enforces_timeout() {
    use std::time::Duration;
    use verdict::audit::{AuditLog, MonitoringServer};
    use verdict::context::PipelineTrace;

    let audit_log = AuditLog::new();
    let trace = PipelineTrace::new();

    eprintln!("TEST: Creating server with timeout 50ms and test delay 200ms");
    // Create server with a 50ms timeout and 200ms test delay (delay > timeout)
    let server = MonitoringServer::with_timeout(audit_log, trace, Duration::from_millis(50))
        .with_test_delay(Duration::from_millis(200));
    eprintln!("TEST: Server created");

    let server_addr: std::net::SocketAddr = "127.0.0.1:19283"
        .parse()
        .expect("failed to parse addr");

    // Spawn server in background
    let server_task = tokio::spawn(async move {
        let _ = server.serve(server_addr).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Create a client with a longer timeout than the server's timeout
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("failed to create client");

    // Make a request to /api/entries. The handler will sleep for 200ms due to test_delay,
    // but the TimeoutLayer will cut the connection after 50ms.
    let result = client
        .get(&format!("http://{}/api/entries", server_addr))
        .send()
        .await;

    // The TimeoutLayer should cut the connection and return 408 Request Timeout
    // or an error. The tower-http TimeoutLayer responds with 408.
    assert!(result.is_ok(), "request should complete but get error response");
    let response = result.unwrap();
    eprintln!("Response status: {}", response.status());
    assert_eq!(
        response.status(),
        408,
        "expected 408 Request Timeout from TimeoutLayer, got {:?}",
        response.status()
    );

    // Cleanup
    server_task.abort();
}

/// Behavioral test: normal requests complete successfully with reasonable timeout
/// Proves the TimeoutLayer doesn't interfere with fast requests
#[tokio::test]
async fn test_monitoring_server_timeout_layer_allows_fast_requests() {
    use std::time::Duration;
    use verdict::audit::{AuditLog, MonitoringServer};
    use verdict::context::PipelineTrace;

    let audit_log = AuditLog::new();
    let trace = PipelineTrace::new();

    // Create server with a reasonable 5-second timeout (handlers should be much faster)
    let server = MonitoringServer::with_timeout(audit_log, trace, Duration::from_secs(5));

    let server_addr: std::net::SocketAddr = "127.0.0.1:19284"
        .parse()
        .expect("failed to parse addr");

    // Spawn server in background
    let server_task = tokio::spawn(async move {
        let _ = server.serve(server_addr).await;
    });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Create a client with a 10-second timeout (longer than server's 5s)
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("failed to create client");

    // Make a request to the simple index handler
    let result = client
        .get(&format!("http://{}/", server_addr))
        .send()
        .await;

    // Should succeed (200 OK)
    assert!(result.is_ok(), "request failed: {:?}", result);
    let response = result.unwrap();
    assert_eq!(response.status(), 200, "expected 200 OK, got {:?}", response.status());

    // Also test the /api/entries endpoint which does real work
    let result = client
        .get(&format!("http://{}/api/entries", server_addr))
        .send()
        .await;
    assert!(result.is_ok(), "entries request failed: {:?}", result);
    assert_eq!(
        result.unwrap().status(),
        200,
        "expected 200 OK for /api/entries"
    );

    // Cleanup
    server_task.abort();
}
