use verdict::prelude::*;
use verdict_app::agent::{
    build_assistant_agent, build_improve_pipeline, build_echo_agent,
    build_memory_agent, build_multi_agent_pipeline, build_eval_pipeline,
};
use verdict_app::memory;
use verdict_app::config::AppConfig;
use serde_json::json;
use std::sync::Arc;

// ============================================================================
// ASSISTANT AGENT TESTS
// ============================================================================

#[test]
fn test_assistant_agent_has_two_steps() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test_agent");

    assert_eq!(agent.pipeline.steps.len(), 2);
    assert_eq!(agent.pipeline.steps[0].name, "understand");
    assert_eq!(agent.pipeline.steps[1].name, "act");
}

#[test]
fn test_understand_step_uses_llm_call() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test_agent");
    let step = &agent.pipeline.steps[0];

    // Check action is LlmCall
    match &step.action {
        StepAction::LlmCall { system, user, .. } => {
            assert!(!system.is_empty());
            assert!(user.contains("{input}"));
        }
        _ => panic!("Expected LlmCall action"),
    }

    // Check guards using pattern matching
    assert!(matches!(step.guard_in, Guard::None));
    assert!(matches!(step.tools, ToolSet::None));
    assert!(matches!(step.injection_protection, InjectionProtection::Strict));
}

#[test]
fn test_act_step_uses_tool_use_loop() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test_agent");
    let step = &agent.pipeline.steps[1];

    // Check action is ToolUseLoop
    match &step.action {
        StepAction::ToolUseLoop {
            system,
            user,
            tools,
            max_rounds,
            ..
        } => {
            assert!(!system.is_empty());
            assert!(user.contains("{understand}") || user.contains("{input}"));
            assert_eq!(*max_rounds, 8);
            assert!(tools.contains(&"fs.read".to_string()));
            assert!(tools.contains(&"shell.run".to_string()));
        }
        _ => panic!("Expected ToolUseLoop action"),
    }

    // Check guard_in requires understand to pass
    match &step.guard_in {
        Guard::StepPassed(step_name) => {
            assert_eq!(step_name, "understand");
        }
        _ => panic!("Expected StepPassed guard_in"),
    }

    // Check guard_out is AllOf with NoSecretsInOutput
    match &step.guard_out {
        Guard::AllOf(guards) => {
            let has_secrets_check = guards.iter().any(|g| {
                matches!(g, Guard::NoSecretsInOutput)
            });
            assert!(has_secrets_check, "Expected NoSecretsInOutput in AllOf");
        }
        _ => panic!("Expected AllOf guard_out"),
    }
}

#[test]
fn test_act_step_guard_out_checks_secrets() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test_agent");
    let step = &agent.pipeline.steps[1];

    // Verify guard_out contains NoSecretsInOutput
    match &step.guard_out {
        Guard::AllOf(guards) => {
            let has_no_secrets = guards.iter().any(|g| {
                matches!(g, Guard::NoSecretsInOutput)
            });
            assert!(has_no_secrets, "Expected NoSecretsInOutput in guard_out");
        }
        _ => panic!("Expected AllOf guard_out"),
    }
}

#[test]
fn test_agent_toolset_is_restricted() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test_agent");

    // Should be an Allow list, not Full
    match &agent.tools {
        ToolSet::Allow(names) => {
            assert_eq!(names.len(), 8);
            assert!(names.contains(&"fs.read".to_string()));
            assert!(names.contains(&"fs.write".to_string()));
            assert!(names.contains(&"search.grep".to_string()));
            assert!(names.contains(&"shell.run".to_string()));
            assert!(names.contains(&"shell.cargo_check".to_string()));
            assert!(names.contains(&"shell.cargo_test".to_string()));
        }
        _ => panic!("Expected ToolSet::Allow"),
    }
}

#[test]
fn test_agent_declares_skills() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test_agent");

    assert_eq!(agent.skills.skills.len(), 5);
    assert!(agent.skills.skills.contains(&"rust_debugging".to_string()));
    assert!(agent.skills.skills.contains(&"code_review".to_string()));
    assert!(agent.skills.skills.contains(&"test_writing".to_string()));
    assert!(agent.skills.skills.contains(&"refactoring".to_string()));
    assert!(agent.skills.skills.contains(&"api_design".to_string()));
}

#[test]
fn test_agent_policy_disallows_self_update() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test_agent");

    assert_eq!(agent.policy.allow_self_update, false);
}

// ============================================================================
// IMPROVE PIPELINE TESTS
// ============================================================================

#[test]
fn test_improve_pipeline_has_two_steps() {
    let pipeline = build_improve_pipeline();

    assert_eq!(pipeline.steps.len(), 2);
    assert_eq!(pipeline.steps[0].name, "self_reflect");
    assert_eq!(pipeline.steps[1].name, "propose_self_update");
}

#[test]
fn test_self_reflect_step_delegates_to_reflector() {
    let pipeline = build_improve_pipeline();
    let step = &pipeline.steps[0];

    match &step.action {
        StepAction::DelegateAgent {
            agent,
            input,
            delegation_policy,
            ..
        } => {
            assert_eq!(agent, "reflector");
            assert!(input.is_object());
            assert_eq!(delegation_policy.max_depth, 1);
            assert!(delegation_policy
                .allowed_agents
                .contains(&"reflector".to_string()));
        }
        _ => panic!("Expected DelegateAgent action"),
    }

    assert!(matches!(step.guard_in, Guard::None));
}

#[test]
fn test_propose_self_update_gates_on_self_reflect() {
    let pipeline = build_improve_pipeline();
    let step = &pipeline.steps[1];

    // Check guard_in requires self_reflect to pass
    match &step.guard_in {
        Guard::StepPassed(step_name) => {
            assert_eq!(step_name, "self_reflect");
        }
        _ => panic!("Expected StepPassed guard_in"),
    }

    // Check action is LlmCall with self_reflect in template
    match &step.action {
        StepAction::LlmCall { user, .. } => {
            assert!(user.contains("{self_reflect}"));
        }
        _ => panic!("Expected LlmCall action"),
    }

    // Check guard_out is AllOf (contains security checks)
    match &step.guard_out {
        Guard::AllOf(guards) => {
            let has_json_check = guards.iter().any(|g| matches!(g, Guard::ValidJson));
            assert!(has_json_check, "Expected ValidJson in guard_out");
        }
        _ => panic!("Expected AllOf guard_out"),
    }

    // Check verdict includes ValidJson
    match &step.verdict {
        Verdict::Automated(Guard::ValidJson) => {
            // Correct!
        }
        _ => panic!("Expected Verdict::Automated(Guard::ValidJson)"),
    }
}

// ============================================================================
// ECHO AGENT TESTS
// ============================================================================

#[test]
fn test_echo_agent_single_step_custom_action() {
    let agent = build_echo_agent("echo_test");

    assert_eq!(agent.pipeline.steps.len(), 1);

    let step = &agent.pipeline.steps[0];
    assert_eq!(step.name, "respond");

    match &step.action {
        StepAction::Custom(_) => {
            // Correct!
        }
        _ => panic!("Expected Custom action"),
    }

    assert!(matches!(agent.tools, ToolSet::None));
    assert!(agent.skills.skills.is_empty());
}

#[tokio::test]
async fn test_echo_agent_custom_action_echoes_input() {
    let agent = build_echo_agent("echo_test");
    let registry = AgentRegistry::new();
    let tool_registry = ToolRegistry::with_builtins();

    let mut runner = PipelineRunner::with_registries(
        Arc::new(tool_registry),
        Arc::new(registry),
    );

    let input = json!("hello world");
    let result = runner
        .run(&agent.pipeline, &agent, input)
        .await
        .expect("Pipeline should succeed");

    assert!(result.success);
    assert!(result.steps_passed.contains(&"respond".to_string()));
    let step_result = result
        .step_results
        .get("respond")
        .expect("Step result should exist");
    assert!(step_result.output.raw.contains("hello world"));
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[test]
fn test_agent_names_are_set_correctly() {
    let config = AppConfig::default();
    let agent1 = build_assistant_agent(&config, "assistant1");
    let agent2 = build_assistant_agent(&config, "assistant2");

    assert_eq!(agent1.name, "assistant1");
    assert_eq!(agent2.name, "assistant2");

    let echo = build_echo_agent("echo_bot");
    assert_eq!(echo.name, "echo_bot");
}

#[test]
fn test_assistant_pipeline_failure_mode_is_abort() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test");

    assert!(matches!(
        agent.pipeline.on_failure,
        FailureMode::Abort
    ));
}

#[test]
fn test_improve_pipeline_failure_mode_is_abort() {
    let pipeline = build_improve_pipeline();

    assert!(matches!(
        pipeline.on_failure,
        FailureMode::Abort
    ));
}

#[test]
fn test_assistant_agent_output_schema_is_none() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test");

    for step in &agent.pipeline.steps {
        assert!(step.output_schema.is_none());
    }
}

#[test]
fn test_agent_pipeline_names_include_agent_name() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "my_agent");

    assert!(agent.pipeline.name.contains("my_agent"));
}

#[test]
fn test_echo_agent_no_dependencies() {
    let agent = build_echo_agent("test");

    for step in &agent.pipeline.steps {
        assert!(step.dependencies.is_empty());
    }
}

#[test]
fn test_assistant_agent_no_parallel_steps() {
    let config = AppConfig::default();
    let agent = build_assistant_agent(&config, "test");

    for step in &agent.pipeline.steps {
        assert!(!step.parallel);
    }
}

#[test]
fn test_improve_pipeline_no_parallel_steps() {
    let pipeline = build_improve_pipeline();

    for step in &pipeline.steps {

// ============================================================================
// NEW PHASE A-F TESTS
// ============================================================================

#[test]
fn test_build_memory_agent_uses_pipeline_builder() {
    let config = AppConfig::default();
    let agent = build_memory_agent(&config, "memory_agent");

    assert_eq!(agent.name, "memory_agent");
    assert_eq!(agent.pipeline.name, "memory-pipeline");
    assert_eq!(agent.pipeline.steps.len(), 2);
    assert!(agent.scorers.len() >= 1, "Memory agent should have at least 1 scorer");
}

#[test]
fn test_memory_agent_has_guard_processors() {
    let config = AppConfig::default();
    let agent = build_memory_agent(&config, "memory_agent");

    let act_step = &agent.pipeline.steps[1];
    assert_eq!(act_step.name, "act");
    assert!(
        act_step.output_processors.len() >= 1,
        "Act step should have at least 1 output processor"
    );
}

#[test]
fn test_multi_agent_pipeline_has_two_steps() {
    let pipeline = build_multi_agent_pipeline("primary", "helper");

    assert_eq!(pipeline.name, "multi-agent-pipeline");
    assert_eq!(pipeline.steps.len(), 2);
    assert_eq!(pipeline.steps[0].name, "delegate_to_helper");
    assert_eq!(pipeline.steps[1].name, "summarize_result");
}

#[test]
fn test_eval_pipeline_uses_rubric_loop() {
    let pipeline = build_eval_pipeline();

    assert_eq!(pipeline.name, "eval-pipeline");
    assert_eq!(pipeline.steps.len(), 1);
    assert_eq!(pipeline.steps[0].name, "evaluate_with_rubric");

    match &pipeline.steps[0].action {
        StepAction::RubricLoop {
            rubric,
            max_iterations,
            ..
        } => {
            assert_eq!(rubric.len(), 2);
            assert_eq!(*max_iterations, 3);
        }
        _ => panic!("Expected RubricLoop action"),
    }
}

#[test]
fn test_memory_store_is_arc_memory_store() {
    let store = memory::build_memory_store();
    // Just verify it's not null and is an Arc
    assert!(Arc::strong_count(&store) >= 1);
}

        assert!(!step.parallel);
    }
}
