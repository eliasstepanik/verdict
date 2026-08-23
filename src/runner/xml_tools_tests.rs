use super::*;
use crate::action::StepOutput;
use crate::cancel::CancellationToken;
use crate::context::{RequestContext, StepContext};
use crate::toolset::ToolSet;
use std::collections::HashMap;

fn make_test_context() -> StepContext {
    StepContext {
        agent_name: "test_agent".into(),
        pipeline_name: "test_pipeline".into(),
        step_name: "test_step".into(),
        step_id: "test-step-1".into(),
        request: serde_json::json!({}),
        input: serde_json::json!("default_input"),
        output: None,
        step_results: HashMap::new(),
        agent_registry: std::sync::Arc::new(crate::registry::AgentRegistry::new()),
        tool_registry: std::sync::Arc::new(crate::registry::ToolRegistry::with_builtins()),
        skill_registry: std::sync::Arc::new(crate::skills::registry::SkillRegistry::new()),
        delegation_depth: 0,
        parent_agent: None,
        allowed_tools: ToolSet::None,
        active_skills: vec![],
        trace: crate::context::PipelineTrace::new(),
        budget: crate::context::BudgetState::default(),
        filesystem_policy: crate::agent::FilesystemPolicy::default(),
        network_policy: crate::agent::NetworkPolicy::DenyAll,
        injection_protection: crate::pipeline::InjectionProtection::None,
        agent_policy: crate::agent::AgentPolicy::default(),
        llm_client: None,
        conversation_history: crate::llm::provider::MessageHistory::new(),
        tools_used: vec![],
        commands_executed: vec![],
        session_meta: None,
        cancellation_token: CancellationToken::new(),
        request_context: RequestContext::default(),
        memory: None,
    }
}

#[test]
fn test_golden_output_correctness() {
    // Test 1: Construct a StepContext with known inputs and step results,
    // call resolve_template with placeholders, and verify correct substitution.
    let mut ctx = make_test_context();
    ctx.input = serde_json::json!("task_value");

    // Add step results with known outputs
    ctx.step_results.insert(
        "step1".into(),
        crate::context::StepResult {
            step_name: "step1".into(),
            output: StepOutput::new("output_from_step1".into()),
            verdict_passed: true,
            error: None,
        },
    );

    ctx.step_results.insert(
        "step2".into(),
        crate::context::StepResult {
            step_name: "step2".into(),
            output: StepOutput::new("output_from_step2".into()),
            verdict_passed: true,
            error: None,
        },
    );

    // Test template with multiple placeholders
    let template = "Input: {input}, Step1: {step1}, Step2: {step2}";
    let result = resolve_template(template, &ctx);

    // Verify all placeholders were substituted correctly
    assert_eq!(
        result,
        "Input: task_value, Step1: output_from_step1, Step2: output_from_step2"
    );

    // Test {input} substitution with object input containing "task" field
    ctx.input = serde_json::json!({ "task": "task_from_object", "other": "value" });
    let template_obj = "Task: {input}";
    let result_obj = resolve_template(template_obj, &ctx);
    assert_eq!(result_obj, "Task: task_from_object");
}

#[test]
fn test_cascading_substitution_prevention() {
    // Test 2: Security test — ensure that step outputs containing literal {placeholder} text
    // are NOT re-scanned and re-substituted (single-pass algorithm prevents cascading).
    let mut ctx = make_test_context();

    // Create a step whose output contains the literal text "{other_step}"
    ctx.step_results.insert(
        "first_step".into(),
        crate::context::StepResult {
            step_name: "first_step".into(),
            output: StepOutput::new("This contains {other_step} literally".into()),
            verdict_passed: true,
            error: None,
        },
    );

    // Also add an actual "other_step" with different content
    ctx.step_results.insert(
        "other_step".into(),
        crate::context::StepResult {
            step_name: "other_step".into(),
            output: StepOutput::new("should_not_appear".into()),
            verdict_passed: true,
            error: None,
        },
    );

    // Template references only first_step, whose output contains {other_step}
    let template = "Result: {first_step}";
    let result = resolve_template(template, &ctx);

    // The output should contain "{other_step}" as a literal string (unexpanded),
    // proving single-pass substitution does not re-scan already-substituted content
    assert_eq!(
        result,
        "Result: This contains {other_step} literally"
    );
    assert!(!result.contains("should_not_appear"), 
            "Cascading substitution should NOT occur");
}

#[test]
fn test_performance_many_substitutions() {
    // Test 3: Performance sanity check — handle 500+ step results efficiently.
    // This verifies the single-pass algorithm scales well.
    let mut ctx = make_test_context();

    // Add 500 step results
    for i in 0..500 {
        let step_name = format!("step_{}", i);
        let output = format!("output_{}", i);
        ctx.step_results.insert(
            step_name.clone(),
            crate::context::StepResult {
                step_name,
                output: StepOutput::new(output),
                verdict_passed: true,
                error: None,
            },
        );
    }

    // Build a template that references many steps
    let mut template = "Processing: ".to_string();
    for i in 0..500 {
        if i > 0 {
            template.push_str(", ");
        }
        template.push_str(&format!("{{step_{}}}", i));
    }

    // Measure performance: should complete well under 100ms
    let start = std::time::Instant::now();
    let result = resolve_template(&template, &ctx);
    let elapsed = start.elapsed();

    // Verify the result contains expected content
    assert!(result.contains("output_0"), "First step output should be present");
    assert!(result.contains("output_499"), "Last step output should be present");

    // Verify performance: should be well under 100ms for single-pass algorithm
    assert!(
        elapsed.as_millis() < 100,
        "resolve_template with 500 steps should complete in <100ms, took {}ms",
        elapsed.as_millis()
    );
}
