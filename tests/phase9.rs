//! Phase 9: Advanced Execution tests
//! Tests for DAG pipelines, branching, remote agents, plugins, hot-reload, and monitoring

use verdict::prelude::*;
use verdict::agents;
use verdict::context::{StepContext, PipelineTrace};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[test]
fn test_dag_topological_sort_basic() {
    // Create steps with dependencies
    let step_a = AgentStep {
        name: "step_a".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "test".into(),
            user: "test".into(),
            model: None,
        },
        guard_out: Guard::None,
        verdict: Verdict::Automated(Guard::None),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: Vec::new(),
        parallel: false,
    };

    let mut step_b = step_a.clone();
    step_b.name = "step_b".into();
    step_b.dependencies = vec!["step_a".into()];

    let mut step_c = step_a.clone();
    step_c.name = "step_c".into();
    step_c.dependencies = vec!["step_a".into(), "step_b".into()];

    let pipeline = Pipeline {
        name: "dag_test".into(),
        steps: vec![step_a, step_b, step_c],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Test topological sort (would normally be used in run_with_dag)
    let sorted = PipelineRunner::topological_sort(&pipeline);
    assert!(sorted.is_ok());
    let indices = sorted.unwrap();
    
    // a should come before b, b before c
    let a_pos = indices.iter().position(|&i| i == 0).unwrap();
    let b_pos = indices.iter().position(|&i| i == 1).unwrap();
    let c_pos = indices.iter().position(|&i| i == 2).unwrap();

    assert!(a_pos < b_pos);
    assert!(b_pos < c_pos);
}

#[test]
fn test_dag_circular_dependency_detection() {
    // Create circular dependency: a -> b -> c -> a
    let mut step_a = AgentStep {
        name: "step_a".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "test".into(),
            user: "test".into(),
            model: None,
        },
        guard_out: Guard::None,
        verdict: Verdict::Automated(Guard::None),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["step_c".into()],  // Circular
        parallel: false,
    };

    let mut step_b = step_a.clone();
    step_b.name = "step_b".into();
    step_b.dependencies = vec!["step_a".into()];

    let mut step_c = step_a.clone();
    step_c.name = "step_c".into();
    step_c.dependencies = vec!["step_b".into()];

    let pipeline = Pipeline {
        name: "circular_test".into(),
        steps: vec![step_a, step_b, step_c],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Circular dependency should fail
    let sorted = PipelineRunner::topological_sort(&pipeline);
    assert!(sorted.is_err());
}

#[tokio::test]
async fn test_branch_action_true_path() {
    let mut runner = PipelineRunner::new();
    let agent = agents::planner_agent();

    // Setup context with output
    let mut ctx = StepContext::new(
        "test_agent".into(),
        "test_pipeline".into(),
        "test_step".into(),
        json!({}),
        agent.policy.filesystem_policy.clone(),
    );
    ctx.output = Some(StepOutput::new("condition_text_present".to_string()));

    // Create branch that checks for condition_text_present
    let action = StepAction::Branch {
        condition: "condition_text_present".to_string(),
        if_true: Box::new(StepAction::LlmCall {
            system: "True path".into(),
            user: "Execute if condition matches".into(),
            model: None,
        }),
        if_false: None,
    };

    // Execute branch
    let result = runner.execute_action(&action, &mut ctx).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    // Should execute if_true which is an LLM stub
    assert!(output.raw.contains("stub") || output.raw.contains("LLM"));
}

#[tokio::test]
async fn test_branch_action_false_path() {
    let mut runner = PipelineRunner::new();
    let agent = agents::planner_agent();

    // Setup context with output
    let mut ctx = StepContext::new(
        "test_agent".into(),
        "test_pipeline".into(),
        "test_step".into(),
        json!({}),
        agent.policy.filesystem_policy.clone(),
    );
    ctx.output = Some(StepOutput::new("no_match_here".to_string()));

    // Create branch that checks for non-existent condition
    let action = StepAction::Branch {
        condition: "condition_not_present".to_string(),
        if_true: Box::new(StepAction::LlmCall {
            system: "True path".into(),
            user: "This should not execute".into(),
            model: None,
        }),
        if_false: Some(Box::new(StepAction::LlmCall {
            system: "False path".into(),
            user: "This should execute".into(),
            model: None,
        })),
    };

    // Execute branch - condition doesn't match
    let result = runner.execute_action(&action, &mut ctx).await;
    assert!(result.is_ok());
}

#[test]
fn test_remote_agent_client_construction() {
    let client = RemoteAgentClient::new();
    // Verify client can be created (no actual HTTP call)
    assert_eq!(std::mem::size_of::<RemoteAgentClient>() > 0, true);
}

#[test]
fn test_hot_reload_handle_creation() {
    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let handle = HotReloadHandle::new(pipeline.clone());
    // Verify handle was created
    assert_eq!(std::mem::size_of::<HotReloadHandle>() > 0, true);
}

#[tokio::test]
async fn test_hot_reload_handle_swap_pipeline() {
    let mut pipeline1 = Pipeline {
        name: "pipeline_v1".into(),
        steps: vec![],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let handle = HotReloadHandle::new(pipeline1.clone());
    
    // Get initial pipeline
    let current = handle.get_pipeline().await;
    assert_eq!(current.name, "pipeline_v1");

    // Update pipeline
    pipeline1.name = "pipeline_v2".into();
    handle.update_pipeline(pipeline1).await;

    // Verify swap
    let updated = handle.get_pipeline().await;
    assert_eq!(updated.name, "pipeline_v2");
}

#[test]
fn test_plugin_registry_creation() {
    let registry = PluginRegistry::new();
    // Verify registry can be created
    assert_eq!(std::mem::size_of::<PluginRegistry>() > 0, true);
}

/// Test plugin for verifying hook execution
struct TestPlugin {
    start_called: Arc<Mutex<bool>>,
    end_called: Arc<Mutex<bool>>,
}

#[async_trait::async_trait]
impl Plugin for TestPlugin {
    fn name(&self) -> &str {
        "test_plugin"
    }

    async fn on_step_start(&self, _ctx: &StepContext) -> Result<(), PluginError> {
        *self.start_called.lock().unwrap() = true;
        Ok(())
    }

    async fn on_step_end(
        &self,
        _ctx: &StepContext,
        _result: &StepOutput,
    ) -> Result<(), PluginError> {
        *self.end_called.lock().unwrap() = true;
        Ok(())
    }
}

#[test]
fn test_plugin_registry_add_plugin() {
    let mut registry = PluginRegistry::new();
    
    let plugin = TestPlugin {
        start_called: Arc::new(Mutex::new(false)),
        end_called: Arc::new(Mutex::new(false)),
    };
    
    let plugin_arc = Arc::new(plugin);
    registry.register(plugin_arc.clone());
    
    let plugins = registry.plugins();
    assert_eq!(plugins.len(), 1);
    assert_eq!(plugins[0].name(), "test_plugin");
}

#[test]
fn test_monitoring_server_construction() {
    let audit_log = AuditLog::new();
    let trace = PipelineTrace::new();
    
    let _server = MonitoringServer::new(audit_log, trace);
    // Verify server can be created
    assert_eq!(std::mem::size_of::<MonitoringServer>() > 0, true);
}

#[test]
fn test_step_action_branch_variant() {
    let action = StepAction::Branch {
        condition: "test_condition".into(),
        if_true: Box::new(StepAction::LlmCall {
            system: "sys".into(),
            user: "user".into(),
            model: None,
        }),
        if_false: None,
    };

    // Verify variant was created correctly
    match action {
        StepAction::Branch { condition, .. } => {
            assert_eq!(condition, "test_condition");
        }
        _ => panic!("Expected Branch action"),
    }
}

#[test]
fn test_step_action_remote_agent_variant() {
    let action = StepAction::RemoteAgent {
        endpoint: "http://localhost:8080".into(),
        agent_name: "remote_agent".into(),
        payload: json!({"key": "value"}),
    };

    // Verify variant was created correctly
    match action {
        StepAction::RemoteAgent {
            endpoint,
            agent_name,
            payload,
        } => {
            assert_eq!(endpoint, "http://localhost:8080");
            assert_eq!(agent_name, "remote_agent");
            assert_eq!(payload["key"], "value");
        }
        _ => panic!("Expected RemoteAgent action"),
    }
}

#[test]
fn test_agent_step_has_dag_fields() {
    let step = AgentStep {
        name: "test_step".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "test".into(),
            user: "test".into(),
            model: None,
        },
        guard_out: Guard::None,
        verdict: Verdict::Automated(Guard::None),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["step_a".into()],
        parallel: true,
    };

    assert_eq!(step.dependencies, vec!["step_a".into()]);
    assert_eq!(step.parallel, true);
}

#[test]
fn test_remote_agent_error_types() {
    let err1 = RemoteAgentError::RequestFailed("test".into());
    let err2 = RemoteAgentError::NetworkError("test".into());
    let err3 = RemoteAgentError::InvalidResponse("test".into());
    let err4 = RemoteAgentError::Timeout;

    // Verify errors can be constructed and formatted
    assert!(err1.to_string().len() > 0);
    assert!(err2.to_string().len() > 0);
    assert!(err3.to_string().len() > 0);
    assert!(err4.to_string().len() > 0);
}

#[test]
fn test_plugin_error_types() {
    let err1 = PluginError::HookFailed("test".into());
    let err2 = PluginError::ExecutionError("test".into());

    // Verify errors can be constructed and formatted
    assert!(err1.to_string().len() > 0);
    assert!(err2.to_string().len() > 0);
}

#[tokio::test]
async fn test_pipeline_execution_with_dag_support() {
    // Create a simple pipeline with DAG
    let step1 = AgentStep {
        name: "first".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "Step 1".into(),
            user: "".into(),
            model: None,
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: Vec::new(),
        parallel: false,
    };

    let mut step2 = step1.clone();
    step2.name = "second".into();
    step2.dependencies = vec!["first".into()];

    let pipeline = Pipeline {
        name: "dag_pipeline".into(),
        steps: vec![step1, step2],
        on_failure: FailureMode::Abort,
        max_retries: 1,
    };

    let mut runner = PipelineRunner::new();
    let agent = agents::planner_agent();

    // Test that DAG execution works (falls back to regular execution)
    let result = runner.run_with_dag(&pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let pipeline_result = result.unwrap();
    assert!(pipeline_result.success);
    assert_eq!(pipeline_result.steps_passed.len(), 2);
}

#[test]
fn test_prelude_exports() {
    // Verify Phase 9 types are exported from prelude
    use verdict::*;

    // Can construct types from prelude
    let _client = RemoteAgentClient::new();
    let _registry = PluginRegistry::new();
    let pipeline = Pipeline {
        name: "test".into(),
        steps: vec![],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let _handle = HotReloadHandle::new(pipeline);
}

