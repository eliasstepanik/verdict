use std::fs;
use std::path::Path;

pub fn handle(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(name);
    
    // Create directory
    fs::create_dir_all(path)?;
    
    // Create Cargo.toml
    let cargo_toml = r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
verdict = "0.1"
tokio = {{ version = "1", features = ["rt", "rt-multi-thread", "macros"] }}
serde_json = "1"
"#.replace("{name}", name);
    fs::write(path.join("Cargo.toml"), cargo_toml)?;
    
    // Create src directory
    fs::create_dir_all(path.join("src"))?;
    
    // Create main.rs
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
    let runner = PipelineRunner::new();
    let result = runner.run(&agent.pipeline, &agent, json!({})).await?;
    
    println!("Pipeline completed: {:?}", result.success);
    
    Ok(())
}
"#;
    fs::write(path.join("src/main.rs"), main_rs)?;
    
    // Create verdict.toml
    let verdict_toml = r#"[project]
name = "{name}"
version = "0.1.0"

[dev]
agent = "hello_agent"
port = 8080
auto_reload = false

[observability]
enabled = false
"#.replace("{name}", name);
    fs::write(path.join("verdict.toml"), verdict_toml)?;
    
    // Create .gitignore
    let gitignore = r#"/target
/Cargo.lock
"#;
    fs::write(path.join(".gitignore"), gitignore)?;
    
    println!("Created new Verdict project: {}", name);
    
    Ok(())
}
