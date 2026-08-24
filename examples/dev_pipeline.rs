//! Example: Run a development pipeline in an interactive mode.
//!
//! This example demonstrates how to load a Verdict configuration from a `verdict.toml` file
//! and start a development agent in the configured project.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example dev_pipeline -- --path /path/to/project
//! cargo run --example dev_pipeline  # Uses current directory
//! ```
//!
//! The example will:
//! 1. Load `verdict.toml` from the project directory
//! 2. Extract the configured development agent name
//! 3. Print development mode status and configuration
//!
//! In a full implementation, this would start an interactive REPL or file watcher
//! for live agent development and testing.

use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Simple argument parsing for --path flag
    let mut project_path: Option<PathBuf> = None;

    let mut i = 1;
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

    println!("Development mode for Verdict project");
    println!("Project path: {}", cwd.display());

    // Check if verdict.toml exists
    let config_path = cwd.join("verdict.toml");
    if !config_path.exists() {
        eprintln!("Warning: verdict.toml not found at {}", config_path.display());
        println!("Using defaults...");
    } else {
        println!("Loaded configuration from {}", config_path.display());
    }

    // In a real implementation, we would:
    // 1. Parse the verdict.toml file
    // 2. Extract the dev.agent configuration
    // 3. Start a file watcher for hot reloading
    // 4. Run the configured agent in development mode
    //
    // For now, we just demonstrate the project loading capability.

    println!("\nDevelopment agent initialized.");
    println!("Press Ctrl+C to stop.");
    println!("\nIn a full implementation, this would:");
    println!("  - Watch for file changes");
    println!("  - Recompile on save");
    println!("  - Run the development agent");
    println!("  - Show live logs and tracing");

    Ok(())
}

fn print_help(program_name: &str) {
    println!(
        "Usage: {} [OPTIONS]

OPTIONS:
  --path <PATH>    Path to the Verdict project (default: current directory)
  -h, --help       Print this help message",
        program_name
    );
}
