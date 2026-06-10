//! Phase 5 — Skills integration tests

use serde_json::json;
use verdict::prelude::*;
use std::sync::Arc;

// ============================================================================
// Helper functions for test pipelines and agents
// ============================================================================

fn skill_pipeline(skill_name: &str, mode: SkillMode) -> Pipeline {
    Pipeline {
        name: "test_skill_pipeline".into(),
        steps: vec![AgentStep {
            name: "use_skill".into(),
            guard_in: Guard::None,
            action: StepAction::UseSkill {
                skill: skill_name.to_string(),
                input: json!({}),
                mode,
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    }
}

fn dummy_agent(name: &str, pipeline: Pipeline) -> Agent {
    Agent {
        name: name.into(),
        description: "".into(),
        pipeline,
        tools: ToolSet::Full,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
    }
}

// ============================================================================
// Tests 1-4: SkillRegistry basic operations
// ============================================================================

#[test]
fn test_skill_registry_starts_empty() {
    let registry = SkillRegistry::new();
    assert!(registry.list().is_empty());
}

#[test]
fn test_skill_registry_register_and_get() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "test_skill".into(),
        description: "A test skill".into(),
        instructions: "Do this: test it".into(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };

    registry.register(skill.clone());
    let retrieved = registry.get("test_skill");
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().name, "test_skill");
}

#[test]
fn test_skill_registry_get_returns_none_for_unknown() {
    let registry = SkillRegistry::new();
    assert!(registry.get("nonexistent").is_none());
}

#[test]
fn test_skill_registry_list_returns_all_names() {
    let mut registry = SkillRegistry::new();
    
    for i in 1..=3 {
        let skill = Skill {
            name: format!("skill_{}", i),
            description: format!("Skill {}", i),
            instructions: format!("Instructions for skill {}", i),
            allowed_tools: ToolSet::Full,
            required_guards: vec![],
            pipeline: None,
            examples: vec![],
            eval: None,
        };
        registry.register(skill);
    }

    let list = registry.list();
    assert_eq!(list.len(), 3);
    assert!(list.contains(&"skill_1".to_string()));
    assert!(list.contains(&"skill_2".to_string()));
    assert!(list.contains(&"skill_3".to_string()));
}

// ============================================================================
// Tests 5-7: Skill and SkillSet struct operations
// ============================================================================

#[test]
fn test_skill_struct_fields_accessible() {
    let skill = Skill {
        name: "my_skill".into(),
        description: "My description".into(),
        instructions: "My instructions".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![Guard::None],
        pipeline: None,
        examples: vec![],
        eval: None,
    };

    assert_eq!(skill.name, "my_skill");
    assert_eq!(skill.description, "My description");
    assert_eq!(skill.instructions, "My instructions");
    assert_eq!(skill.required_guards.len(), 1);
    assert!(skill.examples.is_empty());
}

#[test]
fn test_skillset_construction_from_vec() {
    let skillset = SkillSet::from(vec!["rust", "testing"]);
    assert_eq!(skillset.skills.len(), 2);
    assert!(skillset.skills.contains(&"rust".to_string()));
    assert!(skillset.skills.contains(&"testing".to_string()));
}

#[test]
fn test_skillset_default_is_empty() {
    let skillset = SkillSet::default();
    assert!(skillset.skills.is_empty());
}

// ============================================================================
// Tests 8-12: UseSkill action with various modes
// ============================================================================

#[tokio::test]
async fn test_use_skill_prompt_only_returns_instructions() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "prompt_skill".into(),
        description: "A prompt-only skill".into(),
        instructions: "This is my instruction text".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("prompt_skill", SkillMode::PromptOnly);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 1);
    
    // Check that the output contains the instruction text
    let step_result = result.step_results.get("use_skill");
    assert!(step_result.is_some());
    assert!(step_result.unwrap().output.raw.contains("This is my instruction text"));
}

#[tokio::test]
async fn test_use_skill_unknown_skill_returns_error() {
    let registry = SkillRegistry::new();
    
    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("nonexistent_skill", SkillMode::PromptOnly);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_err());
    // Error should be about skill not found
    match result {
        Err(PipelineError::StepFailed { error, .. }) => {
            assert!(error.to_string().contains("not found"));
        }
        _ => panic!("Expected StepFailed error"),
    }
}

#[tokio::test]
async fn test_use_skill_pipeline_mode_no_pipeline_falls_back() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "no_pipeline_skill".into(),
        description: "A skill without a pipeline".into(),
        instructions: "Fallback to these instructions".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("no_pipeline_skill", SkillMode::Pipeline);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);
    
    let step_result = result.step_results.get("use_skill");
    assert!(step_result.is_some());
    assert!(step_result.unwrap().output.raw.contains("Fallback to these instructions"));
}

#[tokio::test]
async fn test_use_skill_pipeline_mode_with_pipeline_executes_it() {
    let mut registry = SkillRegistry::new();
    
    // Create a simple pipeline for the skill
    let sub_pipeline = Pipeline {
        name: "skill_subpipeline".into(),
        steps: vec![AgentStep {
            name: "execute_work".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(std::sync::Arc::new(|_ctx| {
                Ok(StepOutput::new("skill pipeline executed".into()))
            })),
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let skill = Skill {
        name: "with_pipeline_skill".into(),
        description: "A skill with a pipeline".into(),
        instructions: "Instructions (should not see this)".into(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: Some(sub_pipeline),
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("with_pipeline_skill", SkillMode::Pipeline);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);
    assert_eq!(result.steps_passed.len(), 1);
}

#[tokio::test]
async fn test_use_skill_auto_mode_no_pipeline() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "auto_skill".into(),
        description: "A skill for auto mode testing".into(),
        instructions: "Auto mode instructions".into(),
        allowed_tools: ToolSet::ReadOnly,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let mut runner = PipelineRunner::with_skill_registry(Arc::new(registry));
    let pipeline = skill_pipeline("auto_skill", SkillMode::Auto);
    let agent = dummy_agent("test_agent", pipeline);

    let result = runner.run(&agent.pipeline, &agent, json!({})).await;
    assert!(result.is_ok());
    let result = result.unwrap();
    assert!(result.success);
    
    let step_result = result.step_results.get("use_skill");
    assert!(step_result.is_some());
    assert!(step_result.unwrap().output.raw.contains("Auto mode instructions"));
}

// ============================================================================
// Tests 13-16: Built-in skills validation
// ============================================================================

#[test]
fn test_builtin_rust_debugging_skill_name() {
    let skill = rust_debugging();
    assert_eq!(skill.name, "rust_debugging");
}

#[test]
fn test_builtin_rust_debugging_has_instructions() {
    let skill = rust_debugging();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("cargo check"));
}

#[test]
fn test_builtin_code_review_skill_name() {
    let skill = code_review();
    assert_eq!(skill.name, "code_review");
}

#[test]
fn test_builtin_code_review_has_instructions() {
    let skill = code_review();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("Review"));
}

#[test]
fn test_builtin_api_design_skill_name() {
    let skill = api_design();
    assert_eq!(skill.name, "api_design");
}

#[test]
fn test_builtin_api_design_has_instructions() {
    let skill = api_design();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("API"));
}

// ============================================================================
// Tests 17-18: Multiple skills and runner integration
// ============================================================================

#[test]
fn test_skill_registry_holds_multiple_skills() {
    let mut registry = SkillRegistry::new();
    
    for i in 1..=5 {
        let skill = Skill {
            name: format!("skill_{}", i),
            description: format!("Skill {}", i),
            instructions: format!("Instructions for skill {}", i),
            allowed_tools: ToolSet::Full,
            required_guards: vec![],
            pipeline: None,
            examples: vec![],
            eval: None,
        };
        registry.register(skill);
    }

    // Verify all are retrievable
    for i in 1..=5 {
        let skill_name = format!("skill_{}", i);
        assert!(registry.get(&skill_name).is_some());
        assert_eq!(registry.get(&skill_name).unwrap().name, skill_name);
    }
}

#[test]
fn test_pipeline_runner_with_skill_registry_constructor() {
    let mut registry = SkillRegistry::new();
    let skill = Skill {
        name: "runner_test_skill".into(),
        description: "Test skill".into(),
        instructions: "Test instructions".into(),
        allowed_tools: ToolSet::Full,
        required_guards: vec![],
        pipeline: None,
        examples: vec![],
        eval: None,
    };
    registry.register(skill);

    let registry = Arc::new(registry);
    let runner = PipelineRunner::with_skill_registry(registry.clone());
    
    // Verify the runner was created with the skill registry
    assert_eq!(runner.skill_registry.list().len(), 1);
    assert!(runner.skill_registry.get("runner_test_skill").is_some());
}

#[test]
fn test_skill_example_struct_fields() {
    let example = SkillExample {
        input: json!({ "key": "value" }),
        expected_output: "expected result".into(),
        description: "Example description".into(),
    };

    assert_eq!(example.expected_output, "expected result");
    assert_eq!(example.description, "Example description");
}

#[test]
fn test_skill_eval_struct_fields() {
    let eval = SkillEval {
        criteria: vec!["criterion1".into(), "criterion2".into()],
        min_score: 0.75,
    };

    assert_eq!(eval.criteria.len(), 2);
    assert_eq!(eval.min_score, 0.75);
}

/// Test 109: Built-in test_writing skill has correct name
#[test]
fn test_builtin_test_writing_skill_name() {
    let skill = test_writing();
    assert_eq!(skill.name, "test_writing");
}

/// Test 110: Built-in test_writing skill has non-empty instructions
#[test]
fn test_builtin_test_writing_has_instructions() {
    let skill = test_writing();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("test"));
}

/// Test 111: Built-in refactoring skill has correct name
#[test]
fn test_builtin_refactoring_skill_name() {
    let skill = refactoring();
    assert_eq!(skill.name, "refactoring");
}

/// Test 112: Built-in refactoring skill has non-empty instructions
#[test]
fn test_builtin_refactoring_has_instructions() {
    let skill = refactoring();
    assert!(!skill.instructions.is_empty());
    assert!(skill.instructions.contains("test") || skill.instructions.contains("refactor"));
}


