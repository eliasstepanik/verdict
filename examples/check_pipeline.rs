//! Example: Verify a Verdict project compiles without errors.
//!
//! This example demonstrates how to programmatically run compilation checks
//! on a Verdict project using `cargo check`.
//!
//! # Usage
//!
//! ```sh
//! cargo run --example check_pipeline -- --path /path/to/project
//! cargo run --example check_pipeline  # Checks current directory
//! ```
//!
//! The example will:
//! 1. Navigate to the specified project directory
//! 2. Run `cargo check --all` to verify all crates compile
//! 3. Display compilation output and result status
//!
//! This is useful for:
//! - Pre-commit hooks to catch compilation errors
//! - CI/CD pipelines to validate project integrity
//! - Development workflows to quickly check for compile errors

use std::path::PathBuf;
use std::process::Command;

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

    println!("Running cargo check in: {}", cwd.display());
    println!("Checking all workspace members...\n");

    // Run cargo check --all
    let output = Command::new("cargo")
        .args(&["check", "--all"])
        .current_dir(&cwd)
        .output()?;

    // Print stdout and stderr from cargo check
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !stdout.is_empty() {
        println!("{}", stdout);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        eprintln!("{}", stderr);
    }

    // Report result
    println!("\n{}", "─".repeat(50));
    if output.status.success() {
        println!("✓ All checks passed!");
        println!("The Verdict project compiles successfully.");
    } else {
        println!("✗ Checks failed!");
        println!("Please fix the compilation errors above.");
        std::process::exit(1);
    }

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
