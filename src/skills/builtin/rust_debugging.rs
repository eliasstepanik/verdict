//! Rust debugging skill

use crate::guard::Guard;
use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline};
use crate::skills::skill::Skill;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Create the rust_debugging skill
pub fn rust_debugging() -> Skill {
    Skill {
        name: "rust_debugging".to_string(),
        description: "Find and fix Rust compile/test failures.".to_string(),
        instructions: r#"When debugging Rust:
1. Run cargo check first.
2. Read the compiler error carefully.
3. Fix the smallest possible cause.
4. Run cargo test.
5. Do not rewrite unrelated files.
6. Focus on the root cause, not symptoms."#
            .to_string(),
        allowed_tools: ToolSet::Allow(vec![
            "shell.cargo_check".to_string(),
            "shell.cargo_test".to_string(),
            "fs.read".to_string(),
            "fs.write".to_string(),
        ]),
        required_guards: vec![Guard::Compiles, Guard::TestsPass],
        pipeline: Some(rust_debugging_pipeline()),
        examples: vec![],
        eval: None,
    }
}

/// Helper pipeline for rust_debugging
fn rust_debugging_pipeline() -> Pipeline {
    Pipeline {
        name: "rust_debugging_pipeline".to_string(),
        steps: vec![
            AgentStep {
                name: "check_compilation".to_string(),
                guard_in: Guard::None,
                action: crate::action::StepAction::ToolCall {
                    tool: "shell.cargo_check".to_string(),
                    args: serde_json::json!({}),
                },
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::Allow(vec!["shell.cargo_check".to_string()]),
                injection_protection: InjectionProtection::None,
                output_schema: None,
            },
            AgentStep {
                name: "run_tests".to_string(),
                guard_in: Guard::None,
                action: crate::action::StepAction::ToolCall {
                    tool: "shell.cargo_test".to_string(),
                    args: serde_json::json!({}),
                },
                guard_out: Guard::None,
                verdict: Verdict::Automated(Guard::None),
                tools: ToolSet::Allow(vec!["shell.cargo_test".to_string()]),
                injection_protection: InjectionProtection::None,
                output_schema: None,
            },
        ],
        max_retries: 2,
        on_failure: FailureMode::Abort,
    }
}
