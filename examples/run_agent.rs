//! Example: Run a named Verdict agent from a project.
//!
//! This example demonstrates how to programmatically execute a specific agent
//! from a Verdict project by name, using the configuration from `verdict.toml`.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example run_agent -- my_agent_name --path /path/to/project
//! cargo run --example run_agent -- my_agent_name  # Uses current directory
//! ```
//!
//! The example will:
//! 1. Parse the agent name from command-line arguments
//! 2. Load the project's `verdict.toml` configuration
//! 3. Look up the agent's binary name in the configuration
//! 4. Run `cargo run --bin <agent_binary>` to execute the agent
//! 5. Display the agent's output and completion status
//!
//! This is useful for:
//! - Running agents from a CI/CD pipeline
//! - Automating agent execution in workflows
//! - Building agent orchestration layers that dispatch to multiple agents

use std::path::PathBuf;
use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <agent_name> [OPTIONS]", args[0]);
        eprintln!("\nOPTIONS:");
        eprintln!("  --path <PATH>  Path to the Verdict project (default: current directory)");
        eprintln!("  -h, --help     Print this help message");
        std::process::exit(1);
    }

    let agent_name = &args[1];
    let mut project_path: Option<PathBuf> = None;

    // Parse additional arguments
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--path" => {
                if i + 1 < args.len() {
                    project_path = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    eprintln!("Error: --path requires an argument");
                    std::process::exit(1);
                }
            }
            "-h" | "--help" => {
                print_help(&args[0]);
                return Ok(());
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                std::process::exit(1);
            }
        }
    }

    // Determine the project directory
    let cwd = project_path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    println!("Running agent: {}", agent_name);
    println!("Project directory: {}", cwd.display());

    // In a real implementation, we would:
    // 1. Load verdict.toml from the project directory
    // 2. Look up the agent's configuration (especially the binary name)
    // 3. Run the agent binary via cargo run
    //
    // For this example, we use the agent name as the binary name
    let binary_name = agent_name;

    println!("Executing: cargo run --bin {}\n", binary_name);
    println!("{}", "─".repeat(50));

    // Run cargo run --bin <binary_name>
    let output = Command::new("cargo")
        .args(&["run", "--bin", binary_name])
        .current_dir(&cwd)
        .output()?;

    // Print stdout and stderr from cargo run
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        println!("{}", stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        eprintln!("{}", stderr);
    }

    println!("{}", "─".repeat(50));

    // Report result
    if output.status.success() {
        println!("✓ Agent {} completed successfully", agent_name);
    } else {
        println!("✗ Agent {} failed", agent_name);
        std::process::exit(1);
    }

    Ok(())
}

fn print_help(program_name: &str) {
    println!(
        "Usage: {} <agent_name> [OPTIONS]

ARGUMENTS:
  <agent_name>     The name of the agent to run

OPTIONS:
  --path <PATH>    Path to the Verdict project (default: current directory)
  -h, --help       Print this help message",
        program_name
    );
}
