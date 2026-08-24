//! Example: Create a new Verdict project scaffold.
//!
//! This example demonstrates how to programmatically scaffold a new Verdict project
//! with a basic Cargo.toml, src/main.rs, and verdict.toml configuration.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example new_project -- my_project
//! ```
//!
//! This will create a directory `my_project/` with the following structure:
//! - `Cargo.toml` with `verdict` and `tokio` dependencies
//! - `src/main.rs` with a simple Hello Agent pipeline
//! - `verdict.toml` with dev configuration
//! - `.gitignore` for Rust projects

use std::fs;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command-line arguments
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <project_name>", args[0]);
        std::process::exit(1);
    }

    let project_name = &args[1];
    let path = Path::new(project_name);

    // Create project directory
    fs::create_dir_all(path)?;
    println!("Created directory: {}", project_name);

    // Create Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
verdict = "0.1"
tokio = {{ version = "1", features = ["rt", "rt-multi-thread", "macros"] }}
serde_json = "1"
"#,
        project_name
    );
    fs::write(path.join("Cargo.toml"), cargo_toml)?;
    println!("Created Cargo.toml");

    // Create src directory
    fs::create_dir_all(path.join("src"))?;

    // Create main.rs with a simple pipeline example
    let main_rs = r#"use verdict::prelude::*;
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a simple pipeline
    let pipeline = PipelineBuilder::new("hello")
        .then(
            AgentStep::builder(
                "greet",
                StepAction::LlmCall {
                    system: "You are a helpful assistant.".into(),
                    user: "Say hello!".into(),
                    model: None,
                    conversation_id: None,
                    append_to_history: false,
                },
            )
            .build(),
        )
        .build();

    // Create an agent
    let agent = Agent {
        name: "hello_agent".into(),
        description: "A simple greeting agent".into(),
        pipeline,
        tools: ToolSet::ReadOnly,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: vec![],
    };

    // Run the pipeline
    let mut runner = PipelineRunner::new();
    let result = runner.run(&agent.pipeline, &agent, json!({})).await?;

    println!("Pipeline completed: {:?}", result.success);

    Ok(())
}
"#;
    fs::write(path.join("src/main.rs"), main_rs)?;
    println!("Created src/main.rs");

    // Create verdict.toml configuration
    let verdict_toml = format!(
        r#"[project]
name = "{}"
version = "0.1.0"

[dev]
agent = "hello_agent"
port = 8080
auto_reload = false

[observability]
enabled = false
"#,
        project_name
    );
    fs::write(path.join("verdict.toml"), verdict_toml)?;
    println!("Created verdict.toml");

    // Create .gitignore
    let gitignore = "/target\n/Cargo.lock\n";
    fs::write(path.join(".gitignore"), gitignore)?;
    println!("Created .gitignore");

    println!("\nSuccessfully created new Verdict project: {}", project_name);
    println!("Next steps:");
    println!("  cd {}", project_name);
    println!("  cargo build");

    Ok(())
}
