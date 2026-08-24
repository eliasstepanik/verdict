use verdict::prelude::*;

// Phase E: Observability & Deployment Integration Tests
// This validates the monitoring server enhancements and registry extensions

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
