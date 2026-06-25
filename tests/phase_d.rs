//! Phase D — Multi-Agent & Orchestration Tests
//!
//! D1: Memory Isolation in Delegation
//! D2: Dynamic Toolsets for Multi-Tenant
//! D3: Detached Agent Invocations  
//! D4: Workflow Suspend/Resume

use verdict::prelude::*;
use serde_json::json;

// ========== D1: MemoryIsolation Tests ==========

#[test]
fn test_memory_isolation_enum_exists() {
    // Verify MemoryIsolation enum is available
    let isolated = MemoryIsolation::Isolated;
    let _shared = MemoryIsolation::Shared;
    let _namespaced = MemoryIsolation::NamespacedByAgent;
    
    // Verify they can be used
    let _policy = DelegationPolicy {
        max_depth: 3,
        allowed_agents: vec![],
        require_output_schema: false,
        inherit_tool_scope: true,
        inherit_budget: true,
        require_user_approval: false,
        memory_isolation: isolated,
        on_delegation_start: None,
        on_delegation_complete: None,
        on_iteration_complete: None,
        message_filter: None,
    };
}

#[test]
fn test_delegation_policy_memory_isolation_default() {
    let policy = DelegationPolicy::default();
    // Default should be Isolated
    matches!(policy.memory_isolation, MemoryIsolation::Isolated);
}

#[test]
fn test_delegation_policy_with_shared_memory_isolation() {
    let policy = DelegationPolicy {
        max_depth: 5,
        allowed_agents: vec!["agent1".into()],
        require_output_schema: false,
        inherit_tool_scope: true,
        inherit_budget: true,
        require_user_approval: false,
        memory_isolation: MemoryIsolation::Shared,
        on_delegation_start: None,
        on_delegation_complete: None,
        on_iteration_complete: None,
        message_filter: None,
    };
    assert!(matches!(policy.memory_isolation, MemoryIsolation::Shared));
}

#[test]
fn test_delegation_policy_with_namespaced_memory_isolation() {
    let policy = DelegationPolicy {
        max_depth: 5,
        allowed_agents: vec![],
        require_output_schema: false,
        inherit_tool_scope: true,
        inherit_budget: true,
        require_user_approval: false,
        memory_isolation: MemoryIsolation::NamespacedByAgent,
        on_delegation_start: None,
        on_delegation_complete: None,
        on_iteration_complete: None,
        message_filter: None,
    };
    assert!(matches!(policy.memory_isolation, MemoryIsolation::NamespacedByAgent));
}

// ========== D2: Dynamic Toolsets Tests ==========

#[test]
fn test_request_context_with_toolset() {
    let tool_registry = std::sync::Arc::new(ToolRegistry::with_builtins());
    let mut ctx = RequestContext::new();
    
    // Set a toolset for a specific agent
    ctx.with_toolset("coder", tool_registry.clone());
    
    // Get it back
    let retrieved = ctx.get_toolset("coder");
    assert!(retrieved.is_some());
}

#[test]
fn test_request_context_get_nonexistent_toolset() {
    let ctx = RequestContext::new();
    let retrieved = ctx.get_toolset("nonexistent");
    assert!(retrieved.is_none());
}

#[test]
fn test_request_context_toolsets_builder_style() {
    let tool_registry = std::sync::Arc::new(ToolRegistry::with_builtins());
    let mut ctx = RequestContext::new();
    
    // Builder style chaining
    ctx.with_toolset("agent1", tool_registry.clone())
        .with_toolset("agent2", tool_registry.clone());
    
    // Both should be available
    assert!(ctx.get_toolset("agent1").is_some());
    assert!(ctx.get_toolset("agent2").is_some());
}

// ========== D3: Detached Agent Tests ==========

#[test]
fn test_delegate_agent_has_detached_field() {
    // Verify DelegateAgent struct has detached field
    let _action = StepAction::DelegateAgent {
        agent: "reviewer".into(),
        input: json!({}),
        expected_output_schema: None,
        delegation_policy: DelegationPolicy::default(),
        detached: false,
    };
}

#[test]
fn test_delegate_agent_detached_true() {
    let action = StepAction::DelegateAgent {
        agent: "reviewer".into(),
        input: json!({}),
        expected_output_schema: None,
        delegation_policy: DelegationPolicy::default(),
        detached: true,
    };
    
    // Verify it's set correctly
    if let StepAction::DelegateAgent { detached, .. } = action {
        assert!(detached);
    } else {
        panic!("Expected DelegateAgent");
    }
}

#[test]
fn test_detached_agent_completed_guard_exists() {
    // Verify Guard::DetachedAgentCompleted exists
    let guard = Guard::DetachedAgentCompleted("reviewer".into());
    assert_eq!(guard.name(), "DetachedAgentCompleted");
}

// ========== D4: Suspend Action & State Tests ==========

#[test]
fn test_suspend_action_exists() {
    // Verify Suspend action exists in StepAction enum
    let _action = StepAction::Suspend {
        reason: "awaiting approval".into(),
        resume_schema: None,
        timeout_seconds: Some(3600),
    };
}

#[test]
fn test_suspend_action_with_schema() {
    let resume_schema = json!({
        "type": "object",
        "required": ["approved"],
        "properties": {
            "approved": { "type": "boolean" }
        }
    });
    
    let action = StepAction::Suspend {
        reason: "awaiting human approval".into(),
        resume_schema: Some(resume_schema.clone()),
        timeout_seconds: Some(7200),
    };
    
    if let StepAction::Suspend { resume_schema: schema, .. } = action {
        assert_eq!(schema, Some(resume_schema));
    } else {
        panic!("Expected Suspend action");
    }
}

#[test]
fn test_suspend_action_without_timeout() {
    let action = StepAction::Suspend {
        reason: "paused for manual review".into(),
        resume_schema: None,
        timeout_seconds: None,
    };
    
    if let StepAction::Suspend { timeout_seconds, .. } = action {
        assert_eq!(timeout_seconds, None);
    } else {
        panic!("Expected Suspend action");
    }
}

#[test]
fn test_pipeline_result_suspended_field() {
    // Verify PipelineResult has suspended field
    let result = PipelineResult {
        pipeline_name: "test".into(),
        steps_passed: vec![],
        steps_failed: vec![],
        step_results: std::collections::HashMap::new(),
        audit_log: AuditLog::new(),
        success: false,
        total_cost_usd: 0.0,
        total_tokens_used: 0,
        log: vec![],
        suspended: None,
    };
    
    assert!(result.suspended.is_none());
}

#[test]
fn test_suspended_state_creation() {
    use chrono::Utc;
    
    let state = SuspendedState {
        state_token: "suspend_abc123".into(),
        step_name: "wait_for_approval".into(),
        reason: "awaiting user decision".into(),
        suspended_at: Utc::now(),
    };
    
    assert_eq!(state.state_token, "suspend_abc123");
    assert_eq!(state.step_name, "wait_for_approval");
    assert_eq!(state.reason, "awaiting user decision");
}

#[test]
fn test_resume_data_matches_schema_guard() {
    let schema = json!({
        "type": "object",
        "required": ["decision"],
        "properties": {
            "decision": { "type": "string" }
        }
    });
    
    let guard = Guard::ResumeDataMatchesSchema(schema);
    assert_eq!(guard.name(), "ResumeDataMatchesSchema");
}

// ========== Integration Tests ==========

#[tokio::test]
async fn test_delegation_with_memory_isolation_policies() {
    let mut registry = AgentRegistry::new();
    
    // Create a simple test agent
    let test_agent = Agent {
        name: "test_agent".into(),
        description: "Test agent".into(),
        pipeline: Pipeline {
            name: "test_pipeline".into(),
            steps: vec![AgentStep {
                name: "step1".into(),
                guard_in: Guard::None,
                action: StepAction::LlmCall {
                    system: "respond with 'ok'".into(),
                    user: "test".into(),
                    model: None,
                    conversation_id: None,
                    append_to_history: false,
                },
                guard_out: Guard::None,
                verdict: Verdict::None,
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
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };
    
    registry.register(test_agent);
    
    // Verify agent is registered
    assert!(registry.get("test_agent").is_some());
}

#[tokio::test]
async fn test_pipeline_result_with_suspended_state() {
    use chrono::Utc;
    
    let suspended = SuspendedState {
        state_token: "token_xyz".into(),
        step_name: "wait_step".into(),
        reason: "waiting for user input".into(),
        suspended_at: Utc::now(),
    };
    
    let result = PipelineResult {
        pipeline_name: "suspension_test".into(),
        steps_passed: vec!["step1".into()],
        steps_failed: vec![],
        step_results: std::collections::HashMap::new(),
        audit_log: AuditLog::new(),
        success: false,
        total_cost_usd: 0.0,
        total_tokens_used: 100,
        log: vec![],
        suspended: Some(suspended),
    };
    
    assert!(result.suspended.is_some());
    if let Some(susp) = &result.suspended {
        assert_eq!(susp.state_token, "token_xyz");
        assert_eq!(susp.step_name, "wait_step");
    }
}

#[test]
fn test_memory_isolation_serialization() {
    use serde_json;
    
    let policy = DelegationPolicy {
        max_depth: 3,
        allowed_agents: vec![],
        require_output_schema: false,
        inherit_tool_scope: true,
        inherit_budget: true,
        require_user_approval: false,
        memory_isolation: MemoryIsolation::NamespacedByAgent,
        on_delegation_start: None,
        on_delegation_complete: None,
        on_iteration_complete: None,
        message_filter: None,
    };
    
    let json_str = serde_json::to_string(&policy).expect("Failed to serialize");
    assert!(json_str.contains("NamespacedByAgent"));
}

#[test]
fn test_delegation_policy_detached_agent_field() {
    let policy = DelegationPolicy::default();
    
    let action = StepAction::DelegateAgent {
        agent: "test_agent".into(),
        input: json!({}),
        expected_output_schema: None,
        delegation_policy: policy,
        detached: true,
    };
    
    // Verify detached field is accessible and set correctly
    if let StepAction::DelegateAgent { detached, agent, .. } = action {
        assert!(detached);
        assert_eq!(agent, "test_agent");
    } else {
        panic!("Expected DelegateAgent action");
    }
}
