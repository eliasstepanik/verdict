use std::path::PathBuf;
use std::process::Command;

pub fn handle(path: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = path.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    println!("Running cargo check in {:?}", cwd);

    let output = Command::new("cargo")
        .args(&["check", "--all"])
        .current_dir(&cwd)
        .output()?;

    println!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        println!("{}", String::from_utf8_lossy(&output.stderr));
    }

    if output.status.success() {
        println!("✓ All checks passed");
    } else {
        println!("✗ Checks failed");
    }

    Ok(())
}
