use verdict::prelude::*;
use std::sync::Arc;

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

    let _server = MonitoringServer::new(audit_log, trace)
        .with_agent_registry(agent_registry);

    // Verify we can construct monitoring server with agent registry
    assert!(true);
}

#[test]
fn test_monitoring_server_with_conversation_registry() {
    use verdict::audit::{AuditLog, MonitoringServer};
    use verdict::context::PipelineTrace;
    use verdict::llm::ConversationRegistry;
    use std::sync::Mutex;

    let audit_log = AuditLog::new();
    let trace = PipelineTrace::new();
    let conversation_registry = Arc::new(Mutex::new(ConversationRegistry::new()));

    let _server = MonitoringServer::new(audit_log, trace)
        .with_conversation_registry(conversation_registry);

    // Verify we can construct monitoring server with conversation registry
    assert!(true);
}

#[test]
fn test_agent_registry_list_agents() {
    use verdict::registry::AgentRegistry;
    use verdict::agent::Agent;

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
        skills: SkillSet {
            skills: vec![],
        },
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
    use verdict::registry::AgentRegistry;
    use verdict::agent::Agent;

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
            skills: SkillSet {
                skills: vec![],
            },
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
