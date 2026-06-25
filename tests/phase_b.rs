#![cfg(test)]

use verdict::prelude::*;
use serde_json::json;
use std::sync::Arc;

// Test PipelineBuilder::then
#[test]
fn test_pipeline_builder_then() {
    let step = AgentStep::builder("step1", StepAction::UserInput { prompt: "Enter:".into(), schema: None }).build();
    let pipeline = PipelineBuilder::new("test-pipeline")
        .then(step)
        .build();
    assert_eq!(pipeline.name, "test-pipeline");
    assert_eq!(pipeline.steps.len(), 1);
    assert_eq!(pipeline.steps[0].name, "step1");
    assert!(!pipeline.steps[0].parallel);
}

// Test PipelineBuilder::parallel
#[test]
fn test_pipeline_builder_parallel() {
    let step1 = AgentStep::builder("s1", StepAction::UserInput { prompt: "1".into(), schema: None }).build();
    let step2 = AgentStep::builder("s2", StepAction::UserInput { prompt: "2".into(), schema: None }).build();
    let pipeline = PipelineBuilder::new("par-pipeline")
        .parallel(vec![step1, step2])
        .build();
    assert_eq!(pipeline.steps.len(), 2);
    assert!(pipeline.steps[0].parallel);
    assert!(pipeline.steps[1].parallel);
}

// Test PipelineBuilder::sleep
#[test]
fn test_pipeline_builder_sleep() {
    let pipeline = PipelineBuilder::new("sleep-test")
        .sleep(500)
        .build();
    assert_eq!(pipeline.steps.len(), 1);
    match &pipeline.steps[0].action {
        StepAction::Custom(_) => {
            // Sleep action is implemented as Custom
        }
        _ => panic!("Expected Custom action for sleep"),
    }
}

// Test PipelineBuilder::foreach
#[test]
fn test_pipeline_builder_foreach() {
    let pipeline = PipelineBuilder::new("foreach-test")
        .foreach("items", StepAction::UserInput { prompt: "item".into(), schema: None }, 2)
        .build();
    assert_eq!(pipeline.steps.len(), 1);
    match &pipeline.steps[0].action {
        StepAction::LoopUntil { .. } => {
            // ForEach is implemented as LoopUntil internally
        }
        _ => panic!("Expected LoopUntil action for foreach"),
    }
}

// Test AgentStep::builder defaults
#[test]
fn test_agent_step_builder_defaults() {
    let step = AgentStep::builder("my-step", StepAction::UserInput { prompt: "?".into(), schema: None })
        .build();
    assert_eq!(step.name, "my-step");
    assert!(matches!(step.guard_in, Guard::None));
    assert!(matches!(step.guard_out, Guard::None));
    assert!(matches!(step.verdict, Verdict::None));
    assert!(!step.parallel);
    assert!(step.dependencies.is_empty());
    assert!(step.input_processors.is_empty());
    assert!(step.output_processors.is_empty());
}

// Test AgentStep::builder with guards and options
#[test]
fn test_agent_step_builder_with_guard() {
    let step = AgentStep::builder("guarded", StepAction::UserInput { prompt: "?".into(), schema: None })
        .guard_in(Guard::NonEmptyOutput)
        .guard_out(Guard::MaxLines(100))
        .verdict(Verdict::None)
        .parallel(true)
        .build();
    assert!(matches!(step.guard_in, Guard::NonEmptyOutput));
    assert!(matches!(step.guard_out, Guard::MaxLines(100)));
    assert!(matches!(step.verdict, Verdict::None));
    assert!(step.parallel);
}

// Test RequestContext set/get
#[test]
fn test_request_context_set_get() {
    let ctx = RequestContext::new()
        .set("user_tier", "pro")
        .set("user_id", "42");
    assert_eq!(ctx.get_str("user_tier"), Some("pro"));
    assert_eq!(ctx.get_str("user_id"), Some("42"));
    assert_eq!(ctx.get_str("nonexistent"), None);
}

// Test RequestContext get_bool
#[test]
fn test_request_context_get_bool() {
    let ctx = RequestContext::new().set("is_admin", true);
    assert_eq!(ctx.get_bool("is_admin"), Some(true));
}

// Test RequestContext merge
#[test]
fn test_request_context_merge() {
    let mut ctx1 = RequestContext::new().set("a", "1");
    let ctx2 = RequestContext::new().set("b", "2");
    ctx1.merge(&ctx2);
    assert_eq!(ctx1.get_str("a"), Some("1"));
    assert_eq!(ctx1.get_str("b"), Some("2"));
}

// Test VerdictConfig from_str
#[test]
fn test_verdict_config_from_str() {
    let toml = r#"
[project]
name = "my-agent"
version = "0.1.0"

[dev]
agent = "main"
port = 9090
auto_reload = true
"#;
    let config = VerdictConfig::from_str(toml).unwrap();
    assert_eq!(config.project.name, "my-agent");
    assert_eq!(config.dev.port, Some(9090));
    assert_eq!(config.dev.auto_reload, true);
    assert_eq!(config.dev.agent, Some("main".to_string()));
}

// Test VerdictConfig to_toml
#[test]
fn test_verdict_config_to_toml() {
    let mut config = VerdictConfig::default();
    config.project.name = "test".into();
    config.project.version = "1.0.0".into();
    let toml_str = config.to_toml().unwrap();
    assert!(toml_str.contains("name = \"test\""));
}

// Test GuardProcessor construction
#[test]
fn test_guard_processor_construction() {
    let proc = GuardProcessor::new("pii-check", Guard::NonEmptyOutput)
        .with_strategy(ProcessorStrategy::Warn);
    assert_eq!(proc.name, "pii-check");
    assert!(matches!(proc.strategy, ProcessorStrategy::Warn));
}

// Test GuardProcessor with on_violation callback
#[test]
fn test_guard_processor_on_violation() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let triggered = Arc::new(AtomicBool::new(false));
    let triggered_clone = Arc::clone(&triggered);
    let _proc = GuardProcessor::new("test", Guard::None)
        .with_on_violation(move |_name, _err| {
            triggered_clone.store(true, Ordering::SeqCst);
        });
    assert!(!triggered.load(Ordering::SeqCst));
}

// Test PipelineBuilder::branch
#[test]
fn test_pipeline_builder_branch() {
    let if_true = AgentStep::builder("true-branch", StepAction::UserInput { prompt: "true".into(), schema: None }).build();
    let if_false = AgentStep::builder("false-branch", StepAction::UserInput { prompt: "false".into(), schema: None }).build();
    
    let pipeline = PipelineBuilder::new("branch-test")
        .branch("{condition}", if_true, Some(if_false))
        .build();
    
    assert_eq!(pipeline.steps.len(), 1);
    match &pipeline.steps[0].action {
        StepAction::Branch { condition, if_true: _, if_false: _ } => {
            assert_eq!(condition, "{condition}");
        }
        _ => panic!("Expected Branch action"),
    }
}

// Test PipelineBuilder::on_failure
#[test]
fn test_pipeline_builder_on_failure() {
    let step = AgentStep::builder("s1", StepAction::UserInput { prompt: "?".into(), schema: None }).build();
    let pipeline = PipelineBuilder::new("fail-test")
        .then(step)
        .on_failure(FailureMode::Abort)
        .build();
    
    assert!(matches!(pipeline.on_failure, FailureMode::Abort));
}

// Test PipelineBuilder::max_retries
#[test]
fn test_pipeline_builder_max_retries() {
    let step = AgentStep::builder("s1", StepAction::UserInput { prompt: "?".into(), schema: None }).build();
    let pipeline = PipelineBuilder::new("retry-test")
        .then(step)
        .max_retries(5)
        .build();
    
    assert_eq!(pipeline.max_retries, 5);
}

// Integration test for Phase B features
#[tokio::test]
async fn test_phase_b_integration() {
    // PipelineBuilder with multiple operations
    let step1 = AgentStep::builder("step1", StepAction::UserInput { prompt: "1".into(), schema: None })
        .guard_in(Guard::NonEmptyOutput)
        .build();
    
    let step2 = AgentStep::builder("step2", StepAction::UserInput { prompt: "2".into(), schema: None })
        .depends_on("step1")
        .build();
    
    let pipeline = PipelineBuilder::new("integration-test")
        .then(step1)
        .then(step2)
        .on_failure(FailureMode::Retry)
        .max_retries(2)
        .build();
    
    assert_eq!(pipeline.name, "integration-test");
    assert_eq!(pipeline.steps.len(), 2);
    assert_eq!(pipeline.steps[1].dependencies.len(), 1);
    assert_eq!(pipeline.steps[1].dependencies[0], "step1");
    assert!(matches!(pipeline.on_failure, FailureMode::Retry));
    assert_eq!(pipeline.max_retries, 2);
}

// Test StepAction::UserInput
#[test]
fn test_step_action_user_input() {
    let action = StepAction::UserInput {
        prompt: "What is your name?".into(),
        schema: Some(json!({"type": "string"})),
    };
    
    match action {
        StepAction::UserInput { prompt, schema } => {
            assert_eq!(prompt, "What is your name?");
            assert!(schema.is_some());
        }
        _ => panic!("Expected UserInput action"),
    }
}

// Test GuardProcessor with different strategies
#[test]
fn test_guard_processor_strategies() {
    let warn_proc = GuardProcessor::new("warn-check", Guard::NonEmptyOutput)
        .with_strategy(ProcessorStrategy::Warn);
    assert!(matches!(warn_proc.strategy, ProcessorStrategy::Warn));

    let redact_proc = GuardProcessor::new("redact-check", Guard::ValidJson)
        .with_strategy(ProcessorStrategy::Redact);
    assert!(matches!(redact_proc.strategy, ProcessorStrategy::Redact));

    let rewrite_proc = GuardProcessor::new("rewrite-check", Guard::ValidJson)
        .with_strategy(ProcessorStrategy::Rewrite);
    assert!(matches!(rewrite_proc.strategy, ProcessorStrategy::Rewrite));
}

// Test AgentStep::builder with input/output processors
#[test]
fn test_agent_step_with_processors() {
    let input_proc = GuardProcessor::new("input-check", Guard::NonEmptyOutput)
        .with_strategy(ProcessorStrategy::Warn);
    let output_proc = GuardProcessor::new("output-check", Guard::ValidJson)
        .with_strategy(ProcessorStrategy::Block);
    
    let step = AgentStep::builder("processed-step", StepAction::UserInput { prompt: "?".into(), schema: None })
        .input_processor(input_proc)
        .output_processor(output_proc)
        .build();
    
    assert_eq!(step.input_processors.len(), 1);
    assert_eq!(step.output_processors.len(), 1);
    assert_eq!(step.input_processors[0].name, "input-check");
    assert_eq!(step.output_processors[0].name, "output-check");
}

// Test PipelineBuilder::sleep_until
#[test]
fn test_pipeline_builder_sleep_until() {
    let now = chrono::Utc::now();
    let future = now + chrono::Duration::seconds(10);
    
    let pipeline = PipelineBuilder::new("sleep-until-test")
        .sleep_until(future)
        .build();
    
    assert_eq!(pipeline.steps.len(), 1);
    match &pipeline.steps[0].action {
        StepAction::Custom(_) => {
            // sleep_until is implemented as Custom
        }
        _ => panic!("Expected Custom action"),
    }
}
