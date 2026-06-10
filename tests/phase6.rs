#![allow(unused_imports)]

//! Phase 6 tests: built-in agents
//!
//! Tests for all six specialized agents: planner, coder, reviewer, debugger, reflector, orchestrator.

use verdict::prelude::*;

#[tokio::test]
async fn test_planner_agent_name() {
    let agent = planner_agent();
    assert_eq!(agent.name, "planner");
}

#[tokio::test]
async fn test_planner_agent_description() {
    let agent = planner_agent();
    assert!(!agent.description.is_empty());
    assert_eq!(agent.description, "Produces structured execution plans.");
}

#[tokio::test]
async fn test_planner_agent_pipeline_step() {
    let agent = planner_agent();
    assert!(!agent.pipeline.steps.is_empty());
    assert_eq!(agent.pipeline.steps[0].name, "plan");
}

#[tokio::test]
async fn test_planner_agent_policy_readonly() {
    let agent = planner_agent();
    match &agent.tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.tools),
    }
    match &agent.policy.allowed_tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.policy.allowed_tools),
    }
}

#[tokio::test]
async fn test_planner_agent_no_self_update() {
    let agent = planner_agent();
    assert!(!agent.policy.allow_self_update);
}

#[tokio::test]
async fn test_coder_agent_name() {
    let agent = coder_agent();
    assert_eq!(agent.name, "coder");
}

#[tokio::test]
async fn test_coder_agent_description() {
    let agent = coder_agent();
    assert!(!agent.description.is_empty());
    assert_eq!(agent.description, "Implements approved software changes.");
}

#[tokio::test]
async fn test_coder_agent_pipeline_step() {
    let agent = coder_agent();
    assert!(!agent.pipeline.steps.is_empty());
    assert_eq!(agent.pipeline.steps[0].name, "implement");
}

#[tokio::test]
async fn test_coder_agent_policy_readwrite() {
    let agent = coder_agent();
    match &agent.tools {
        ToolSet::ReadWrite => (),
        _ => panic!("Expected ReadWrite, got {:?}", agent.tools),
    }
    match &agent.policy.allowed_tools {
        ToolSet::ReadWrite => (),
        _ => panic!("Expected ReadWrite, got {:?}", agent.policy.allowed_tools),
    }
}

#[tokio::test]
async fn test_coder_agent_skills() {
    let agent = coder_agent();
    assert!(agent.skills.skills.contains(&"rust_debugging".to_string()));
    assert!(agent.skills.skills.contains(&"test_writing".to_string()));
}

#[tokio::test]
async fn test_coder_agent_no_self_update() {
    let agent = coder_agent();
    assert!(!agent.policy.allow_self_update);
}

#[tokio::test]
async fn test_reviewer_agent_name() {
    let agent = reviewer_agent();
    assert_eq!(agent.name, "reviewer");
}

#[tokio::test]
async fn test_reviewer_agent_description() {
    let agent = reviewer_agent();
    assert!(!agent.description.is_empty());
    assert_eq!(
        agent.description,
        "Reviews code changes for quality, safety, and correctness."
    );
}

#[tokio::test]
async fn test_reviewer_agent_pipeline_step() {
    let agent = reviewer_agent();
    assert!(!agent.pipeline.steps.is_empty());
    assert_eq!(agent.pipeline.steps[0].name, "review");
}

#[tokio::test]
async fn test_reviewer_agent_policy_readonly() {
    let agent = reviewer_agent();
    match &agent.tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.tools),
    }
    match &agent.policy.allowed_tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.policy.allowed_tools),
    }
}

#[tokio::test]
async fn test_reviewer_agent_no_self_update() {
    let agent = reviewer_agent();
    assert!(!agent.policy.allow_self_update);
}

#[tokio::test]
async fn test_debugger_agent_name() {
    let agent = debugger_agent();
    assert_eq!(agent.name, "debugger");
}

#[tokio::test]
async fn test_debugger_agent_description() {
    let agent = debugger_agent();
    assert!(!agent.description.is_empty());
    assert_eq!(
        agent.description,
        "Diagnoses and fixes compile and test failures."
    );
}

#[tokio::test]
async fn test_debugger_agent_pipeline_step() {
    let agent = debugger_agent();
    assert!(!agent.pipeline.steps.is_empty());
    assert_eq!(agent.pipeline.steps[0].name, "debug");
}

#[tokio::test]
async fn test_debugger_agent_policy_readwrite() {
    let agent = debugger_agent();
    match &agent.tools {
        ToolSet::ReadWrite => (),
        _ => panic!("Expected ReadWrite, got {:?}", agent.tools),
    }
    match &agent.policy.allowed_tools {
        ToolSet::ReadWrite => (),
        _ => panic!("Expected ReadWrite, got {:?}", agent.policy.allowed_tools),
    }
}

#[tokio::test]
async fn test_debugger_agent_no_self_update() {
    let agent = debugger_agent();
    assert!(!agent.policy.allow_self_update);
}

#[tokio::test]
async fn test_reflector_agent_name() {
    let agent = reflector_agent();
    assert_eq!(agent.name, "reflector");
}

#[tokio::test]
async fn test_reflector_agent_description() {
    let agent = reflector_agent();
    assert!(!agent.description.is_empty());
    assert_eq!(
        agent.description,
        "Analyzes agent performance and suggests improvements."
    );
}

#[tokio::test]
async fn test_reflector_agent_pipeline_step() {
    let agent = reflector_agent();
    assert!(!agent.pipeline.steps.is_empty());
    assert_eq!(agent.pipeline.steps[0].name, "reflect");
}

#[tokio::test]
async fn test_reflector_agent_policy_readonly() {
    let agent = reflector_agent();
    match &agent.tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.tools),
    }
    match &agent.policy.allowed_tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.policy.allowed_tools),
    }
}

#[tokio::test]
async fn test_reflector_agent_no_self_update() {
    let agent = reflector_agent();
    assert!(!agent.policy.allow_self_update);
}

#[tokio::test]
async fn test_orchestrator_agent_name() {
    let agent = orchestrator_agent();
    assert_eq!(agent.name, "orchestrator");
}

#[tokio::test]
async fn test_orchestrator_agent_description() {
    let agent = orchestrator_agent();
    assert!(!agent.description.is_empty());
    assert_eq!(
        agent.description,
        "Delegates work to specialized agents to achieve user goals."
    );
}

#[tokio::test]
async fn test_orchestrator_agent_pipeline_step() {
    let agent = orchestrator_agent();
    assert!(!agent.pipeline.steps.is_empty());
    assert_eq!(agent.pipeline.steps[0].name, "orchestrate");
}

#[tokio::test]
async fn test_orchestrator_agent_policy_readonly() {
    let agent = orchestrator_agent();
    match &agent.tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.tools),
    }
    match &agent.policy.allowed_tools {
        ToolSet::ReadOnly => (),
        _ => panic!("Expected ReadOnly, got {:?}", agent.policy.allowed_tools),
    }
}

#[tokio::test]
async fn test_orchestrator_agent_allowed_agents() {
    let agent = orchestrator_agent();
    assert!(agent.policy.allowed_agents.contains(&"planner".to_string()));
    assert!(agent.policy.allowed_agents.contains(&"coder".to_string()));
    assert!(agent.policy.allowed_agents.contains(&"reviewer".to_string()));
    assert!(agent.policy.allowed_agents.contains(&"debugger".to_string()));
    assert!(agent.policy.allowed_agents.contains(&"reflector".to_string()));
}

#[tokio::test]
async fn test_orchestrator_agent_delegation_depth() {
    let agent = orchestrator_agent();
    assert_eq!(agent.policy.max_delegation_depth, 3);
}

#[tokio::test]
async fn test_orchestrator_agent_no_self_update() {
    let agent = orchestrator_agent();
    assert!(!agent.policy.allow_self_update);
}

#[tokio::test]
async fn test_all_agents_can_register() {
    let mut registry = AgentRegistry::new();
    let planner = planner_agent();
    let coder = coder_agent();
    let reviewer = reviewer_agent();
    let debugger = debugger_agent();
    let reflector = reflector_agent();
    let orchestrator = orchestrator_agent();

    registry.register(planner);
    registry.register(coder);
    registry.register(reviewer);
    registry.register(debugger);
    registry.register(reflector);
    registry.register(orchestrator);

    assert_eq!(registry.list().len(), 6);
}

#[tokio::test]
async fn test_agent_registry_contains_all_agents() {
    let mut registry = AgentRegistry::new();
    registry.register(planner_agent());
    registry.register(coder_agent());
    registry.register(reviewer_agent());
    registry.register(debugger_agent());
    registry.register(reflector_agent());
    registry.register(orchestrator_agent());

    let agent_names = registry.list();

    assert!(agent_names.contains(&"planner".to_string()));
    assert!(agent_names.contains(&"coder".to_string()));
    assert!(agent_names.contains(&"reviewer".to_string()));
    assert!(agent_names.contains(&"debugger".to_string()));
    assert!(agent_names.contains(&"reflector".to_string()));
    assert!(agent_names.contains(&"orchestrator".to_string()));
}

#[tokio::test]
async fn test_all_agents_have_description() {
    let agents = vec![
        planner_agent(),
        coder_agent(),
        reviewer_agent(),
        debugger_agent(),
        reflector_agent(),
        orchestrator_agent(),
    ];

    for agent in agents {
        assert!(!agent.description.is_empty(), "Agent {} has empty description", agent.name);
    }
}

#[tokio::test]
async fn test_planner_agent_delegation_depth() {
    let agent = planner_agent();
    assert_eq!(agent.policy.max_delegation_depth, 1);
}

#[tokio::test]
async fn test_coder_agent_delegation_depth() {
    let agent = coder_agent();
    assert_eq!(agent.policy.max_delegation_depth, 2);
}

#[tokio::test]
async fn test_reviewer_agent_delegation_depth() {
    let agent = reviewer_agent();
    assert_eq!(agent.policy.max_delegation_depth, 1);
}

#[tokio::test]
async fn test_debugger_agent_delegation_depth() {
    let agent = debugger_agent();
    assert_eq!(agent.policy.max_delegation_depth, 1);
}

#[tokio::test]
async fn test_reflector_agent_delegation_depth() {
    let agent = reflector_agent();
    assert_eq!(agent.policy.max_delegation_depth, 1);
}
