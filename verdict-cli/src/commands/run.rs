use std::path::PathBuf;
use std::process::Command;
use verdict::VerdictConfig;

pub fn handle(agent: &str, path: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // Load verdict.toml
    let config_path = cwd.join("verdict.toml");
    let config = if config_path.exists() {
        VerdictConfig::from_file(&config_path)?
    } else {
        VerdictConfig::default()
    };

    // Get the binary name for the agent
    let agent_config = config.agents.get(agent);
    let binary_name = agent_config
        .and_then(|c| c.binary.as_ref())
        .map(|b| b.as_str())
        .unwrap_or(agent);

    println!("Running agent: {}", agent);
    println!("Binary: {}", binary_name);

    let output = Command::new("cargo")
        .args(&["run", "--bin", binary_name])
        .current_dir(&cwd)
        .output()?;

    println!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        println!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        println!("✓ Agent completed");
    } else {
        println!("✗ Agent failed");
    }

    Ok(())
}
