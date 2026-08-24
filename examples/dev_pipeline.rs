//! Example: Build and run a simple Verdict pipeline.
//!
//! This example demonstrates how to construct a multi-step pipeline using the Verdict library,
//! where each step transforms data and passes it to the next step via `ctx.step_results`.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example dev_pipeline
//! ```
//!
//! The example will:
//! 1. Create a 3-step pipeline that transforms input data
//! 2. Build an agent with that pipeline
//! 3. Run the pipeline and print real output from each step
//! 4. Demonstrate data flow between steps via step results

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create step 1: Echo input and add "-step1"
    let step1 = AgentStep {
        name: "transform_a".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            Ok(StepOutput::new("input".into()))
        })),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    // Create step 2: Take step1's output and append "-step2"
    let step2 = AgentStep {
        name: "transform_b".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|ctx| {
            let prev = ctx
                .step_results
                .get("transform_a")
                .ok_or_else(|| StepError::ActionFailed {
                    reason: "transform_a not found".into(),
                })?
                .output
                .raw
                .clone();
            Ok(StepOutput::new(format!("{prev}-step2")))
        })),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["transform_a".into()],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    // Create step 3: Take step2's output and append "-step3"
    let step3 = AgentStep {
        name: "transform_c".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|ctx| {
            let prev = ctx
                .step_results
                .get("transform_b")
                .ok_or_else(|| StepError::ActionFailed {
                    reason: "transform_b not found".into(),
                })?
                .output
                .raw
                .clone();
            Ok(StepOutput::new(format!("{prev}-step3")))
        })),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["transform_b".into()],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    // Build the pipeline
    let pipeline = Pipeline {
        name: "example_pipeline".into(),
        steps: vec![step1, step2, step3],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Create an agent with this pipeline
    let agent = Agent {
        name: "example_agent".into(),
        description: "Example agent demonstrating pipeline execution".into(),
        pipeline,
        tools: ToolSet::None,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    // Create a pipeline runner and execute
    let mut runner = PipelineRunner::new();
    let result = runner.run(&agent.pipeline, &agent, json!({})).await?;

    // Print results
    println!("═══════════════════════════════════════════");
    println!("Pipeline Execution Results");
    println!("═══════════════════════════════════════════\n");
    println!("Pipeline: {}", agent.pipeline.name);
    println!("Status: {}", if result.success { "✓ SUCCESS" } else { "✗ FAILED" });
    println!("Steps passed: {}", result.steps_passed.join(", "));

    if !result.steps_failed.is_empty() {
        println!("Steps failed: {}", result.steps_failed.join(", "));
    }

    println!("\n───────────────────────────────────────────");
    println!("Step Outputs:");
    println!("───────────────────────────────────────────");

    for step_name in &result.steps_passed {
        if let Some(step_result) = result.step_results.get(step_name.as_str()) {
            println!("  {}: {}", step_name, step_result.output.raw);
        }
    }

    println!("\n═══════════════════════════════════════════");
    println!("Final Output: {}",
             result.step_results
                 .get("transform_c")
                 .map(|sr| &sr.output.raw)
                 .unwrap_or(&"(not available)".to_string()));
    println!("═══════════════════════════════════════════");

    Ok(())
}
