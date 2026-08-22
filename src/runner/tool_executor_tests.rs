use super::extract_shell_command_string;

#[test]
fn test_extract_shell_run_command() {
    let args = serde_json::json!({
        "command": "rm",
        "args": ["-rf", "/tmp"]
    });
    let result = extract_shell_command_string("shell.run", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "rm -rf /tmp");
}

#[test]
fn test_extract_shell_run_command_tool_run_command_variant() {
    // Critical test: shell.run_command must extract the command the same way as shell.run
    let args = serde_json::json!({
        "command": "rm",
        "args": ["-rf", "/tmp"]
    });
    let result = extract_shell_command_string("shell.run_command", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "rm -rf /tmp");
}

#[test]
fn test_extract_shell_cargo_test() {
    let args = serde_json::json!({});
    let result = extract_shell_command_string("shell.cargo_test", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cargo test");
}

#[test]
fn test_extract_shell_unknown_fallback() {
    let args = serde_json::json!({});
    let result = extract_shell_command_string("shell.custom_tool", &args);
    assert!(result.is_ok());
    // Should strip the "shell." prefix
    assert_eq!(result.unwrap(), "custom_tool");
}

#[test]
fn test_extract_shell_run_command_with_no_args() {
    let args = serde_json::json!({
        "command": "cargo"
    });
    let result = extract_shell_command_string("shell.run_command", &args);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "cargo");
}
