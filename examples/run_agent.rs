//! Example: Instantiate and run a Verdict agent.
//!
//! This example demonstrates how to programmatically create an agent with a custom pipeline,
//! run it via `PipelineRunner::run()`, and observe the real pipeline execution and output.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example run_agent
//! ```
//!
//! The example will:
//! 1. Create a custom agent with a multi-step pipeline
//! 2. Each step performs a simple validation or transformation
//! 3. Run the agent's pipeline via `PipelineRunner::run()`
//! 4. Print the real output and pipeline execution details

use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("═══════════════════════════════════════════════════════════");
    println!("Verdict Agent Execution Example");
    println!("═══════════════════════════════════════════════════════════\n");

    // Create step 1: Validate input
    let validate_input = AgentStep {
        name: "validate_input".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|_ctx| {
            let result = "Input validation: OK ✓";
            Ok(StepOutput::new(result.into()))
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

    // Create step 2: Process the request
    let process_request = AgentStep {
        name: "process_request".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|ctx| {
            let _prev = ctx
                .step_results
                .get("validate_input")
                .ok_or_else(|| StepError::ActionFailed {
                    reason: "validate_input not found".into(),
                })?
                .output
                .raw
                .clone();
            let result = "Request processing: Building execution plan";
            Ok(StepOutput::new(result.into()))
        })),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["validate_input".into()],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    // Create step 3: Generate output
    let generate_output = AgentStep {
        name: "generate_output".into(),
        guard_in: Guard::None,
        action: StepAction::Custom(Arc::new(|ctx| {
            let _prev = ctx
                .step_results
                .get("process_request")
                .ok_or_else(|| StepError::ActionFailed {
                    reason: "process_request not found".into(),
                })?
                .output
                .raw
                .clone();
            let result = "Output generation: Plan ready for execution";
            Ok(StepOutput::new(result.into()))
        })),
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::Automated(Guard::NonEmptyOutput),
        tools: ToolSet::None,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec!["process_request".into()],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    // Build the agent's pipeline
    let pipeline = Pipeline {
        name: "example_agent_pipeline".into(),
        steps: vec![validate_input, process_request, generate_output],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Create the agent
    let agent = Agent {
        name: "example_agent".into(),
        description: "Example agent demonstrating real agent execution with a custom pipeline".into(),
        pipeline,
        tools: ToolSet::ReadOnly,
        skills: SkillSet::default(),
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    println!("Agent: {}", agent.name);
    println!("Description: {}", agent.description);
    println!("\n───────────────────────────────────────────────────────────");
    println!("Pipeline: {}", agent.pipeline.name);
    println!("Steps: {}", agent.pipeline.steps.len());

    // Prepare input for the agent
    let input = json!({
        "request": "Implement a command-line calculator"
    });

    println!("\nInput:");
    println!("{}", serde_json::to_string_pretty(&input)?);

    // Create a pipeline runner
    let mut runner = PipelineRunner::new();

    println!("\n───────────────────────────────────────────────────────────");
    println!("Running agent pipeline...\n");

    // Run the agent's pipeline
    let result = runner
        .run(&agent.pipeline, &agent, input)
        .await?;

    // Report execution results
    println!("\n───────────────────────────────────────────────────────────");
    println!("Pipeline Execution Results:");
    println!("───────────────────────────────────────────────────────────");
    println!("Status: {}", if result.success { "✓ SUCCESS" } else { "✗ FAILED" });
    println!("Steps passed: {:?}", result.steps_passed);

    if !result.steps_failed.is_empty() {
        println!("Steps failed: {:?}", result.steps_failed);
    }

    // Print step outputs
    println!("\n───────────────────────────────────────────────────────────");
    println!("Step Outputs:");
    println!("───────────────────────────────────────────────────────────");

    for step_name in &result.steps_passed {
        if let Some(step_result) = result.step_results.get(step_name.as_str()) {
            println!("\n  {}: {}", step_name, step_result.output.raw);
        }
    }

    println!("\n═══════════════════════════════════════════════════════════");
    println!("Agent execution completed successfully!");
    println!("═══════════════════════════════════════════════════════════");

    Ok(())
}
