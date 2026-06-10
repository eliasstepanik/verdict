use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::path::PathBuf;
use verdict::prelude::*;
use serde_json::{json, Value};

// Helper: get workspace root (project root)
fn workspace_root() -> PathBuf {
    std::env::current_dir().unwrap()
}

// Helper: create unique temp filename
fn unique_temp_file(suffix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!("verdict_test_{}_{}.txt", nanos, suffix)
}

#[tokio::test]
async fn test_tool_registry_new_is_empty() {
    let registry = ToolRegistry::new();
    let list = registry.list();
    assert_eq!(list.len(), 0);
}

#[tokio::test]
async fn test_tool_registry_register_and_get() {
    let echo_tool = FunctionTool::new(
        "test.echo",
        "Echo input",
        json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            },
            "required": ["message"]
        }),
        |args, _ctx| {
            Box::pin(async move {
                let msg = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("empty");
                Ok(ToolOutput::text(msg.to_string()))
            })
        },
    );

    let mut registry = ToolRegistry::new();
    registry.register(echo_tool);

    let retrieved = registry.get("test.echo");
    assert!(retrieved.is_some());

    let list = registry.list();
    assert_eq!(list.len(), 1);
    assert!(list.contains(&"test.echo".to_string()));
}

#[tokio::test]
async fn test_tool_registry_with_builtins() {
    let registry = ToolRegistry::with_builtins();
    let list = registry.list();

    // Should have at least the standard built-in tools
    assert!(list.contains(&"fs.read".to_string()));
    assert!(list.contains(&"fs.write".to_string()));
    assert!(list.contains(&"fs.list".to_string()));
    assert!(list.contains(&"fs.delete".to_string()));
    assert!(list.contains(&"shell.cargo_check".to_string()));
    assert!(list.contains(&"shell.cargo_test".to_string()));
    assert!(list.contains(&"shell.cargo_fmt".to_string()));
    assert!(list.contains(&"shell.run_command".to_string()));
    assert!(list.contains(&"search.files".to_string()));
    assert!(list.contains(&"search.grep".to_string()));

    // Count should be at least 10
    assert!(list.len() >= 10);
}

#[tokio::test]
async fn test_function_tool_creation_and_call() {
    let multiply_tool = FunctionTool::new(
        "math.multiply",
        "Multiply two numbers",
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" }
            },
            "required": ["a", "b"]
        }),
        |args, _ctx| {
            Box::pin(async move {
                let a = args.get("a").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let b = args.get("b").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let result = a * b;
                Ok(ToolOutput::json(json!({ "result": result })))
            })
        },
    );

    assert_eq!(multiply_tool.name(), "math.multiply");
    assert_eq!(multiply_tool.description(), "Multiply two numbers");

    let ctx = ToolContext {
        filesystem_policy: FilesystemPolicy::default(),
        network_policy: NetworkPolicy::DenyAll,
        allowed_tools: ToolSet::Full,
        audit_log: Arc::new(Mutex::new(AuditLog::new())),
    };

    let result = multiply_tool
        .call(json!({ "a": 3.0, "b": 4.0 }), ctx)
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.raw.contains("12"));
}

#[tokio::test]
async fn test_fs_read_cargo_toml() {
    let registry = ToolRegistry::with_builtins();
    let fs_read = registry.get("fs.read").expect("fs.read not found");

    let mut fs_policy = FilesystemPolicy::default();
    fs_policy.workspace_root = workspace_root();

    let ctx = ToolContext {
        filesystem_policy: fs_policy,
        network_policy: NetworkPolicy::DenyAll,
        allowed_tools: ToolSet::ReadOnly,
        audit_log: Arc::new(Mutex::new(AuditLog::new())),
    };

    let result = fs_read
        .call(json!({ "path": "Cargo.toml" }), ctx)
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.raw.contains("verdict"));
}

#[tokio::test]
async fn test_fs_read_rejects_path_escape() {
    let registry = ToolRegistry::with_builtins();
    let fs_read = registry.get("fs.read").expect("fs.read not found");

    let mut fs_policy = FilesystemPolicy::default();
    fs_policy.workspace_root = workspace_root();

    let ctx = ToolContext {
        filesystem_policy: fs_policy,
        network_policy: NetworkPolicy::DenyAll,
        allowed_tools: ToolSet::ReadOnly,
        audit_log: Arc::new(Mutex::new(AuditLog::new())),
    };

    // Try to escape workspace
    let result = fs_read
        .call(json!({ "path": "../../etc/passwd" }), ctx)
        .await;

    // Should fail or at least not return actual /etc/passwd
    // (failure is expected)
    let is_failure_or_safe = result.is_err() || !result.unwrap().raw.contains("root:x:");
    assert!(is_failure_or_safe);
}

#[tokio::test]
async fn test_fs_write_and_read_roundtrip() {
    let registry = ToolRegistry::with_builtins();
    let fs_write = registry.get("fs.write").expect("fs.write not found");
    let fs_read = registry.get("fs.read").expect("fs.read not found");

    let temp_dir = std::env::temp_dir();
    let temp_filename = unique_temp_file("roundtrip");
    
    let mut fs_policy = FilesystemPolicy::default();
    fs_policy.workspace_root = temp_dir.clone();

    let ctx = ToolContext {
        filesystem_policy: fs_policy,
        network_policy: NetworkPolicy::DenyAll,
        allowed_tools: ToolSet::ReadWrite,
        audit_log: Arc::new(Mutex::new(AuditLog::new())),
    };

    let test_content = "Hello, Verdict Phase 2!";

    // Write
    let write_result = fs_write
        .call(
            json!({ "path": &temp_filename, "content": test_content }),
            ctx.clone(),
        )
        .await;
    assert!(write_result.is_ok());

    // Read back
    let read_result = fs_read
        .call(json!({ "path": &temp_filename }), ctx)
        .await;
    assert!(read_result.is_ok());
    let output = read_result.unwrap();
    assert_eq!(output.raw, test_content);

    // Cleanup
    std::fs::remove_file(temp_dir.join(&temp_filename)).ok();
}

#[tokio::test]
async fn test_fs_list_directory() {
    let registry = ToolRegistry::with_builtins();
    let fs_list = registry.get("fs.list").expect("fs.list not found");

    let mut fs_policy = FilesystemPolicy::default();
    fs_policy.workspace_root = workspace_root();

    let ctx = ToolContext {
        filesystem_policy: fs_policy,
        network_policy: NetworkPolicy::DenyAll,
        allowed_tools: ToolSet::ReadOnly,
        audit_log: Arc::new(Mutex::new(AuditLog::new())),
    };

    let result = fs_list
        .call(json!({ "path": "." }), ctx)
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    
    // Check that output is valid JSON
    let parsed = serde_json::from_str::<Value>(&output.raw);
    assert!(parsed.is_ok());
    
    let entries = parsed.unwrap();
    let entries_array = entries.get("entries").and_then(|e| e.as_array());
    assert!(entries_array.is_some());
    assert!(!entries_array.unwrap().is_empty());
}

#[tokio::test]
async fn test_search_grep_in_cargo_toml() {
    let registry = ToolRegistry::with_builtins();
    let grep = registry.get("search.grep").expect("search.grep not found");

    let mut fs_policy = FilesystemPolicy::default();
    fs_policy.workspace_root = workspace_root();

    let ctx = ToolContext {
        filesystem_policy: fs_policy,
        network_policy: NetworkPolicy::DenyAll,
        allowed_tools: ToolSet::ReadOnly,
        audit_log: Arc::new(Mutex::new(AuditLog::new())),
    };

    let result = grep
        .call(
            json!({ "pattern": "verdict", "path": "Cargo.toml", "recursive": false }),
            ctx,
        )
        .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    
    // Parse JSON response
    let parsed = serde_json::from_str::<Value>(&output.raw);
    assert!(parsed.is_ok());
    
    let json_obj = parsed.unwrap();
    let matches = json_obj.get("matches").and_then(|m| m.as_array());
    assert!(matches.is_some());
    assert!(!matches.unwrap().is_empty());
}

#[tokio::test]
async fn test_toolset_readonly_enforcement() {
    let readonly = ToolSet::ReadOnly;

    // Should allow read tools
    assert!(readonly.contains("fs.read"));
    assert!(readonly.contains("fs.list"));
    assert!(readonly.contains("search.files"));
    assert!(readonly.contains("search.grep"));

    // Should NOT allow write tools
    assert!(!readonly.contains("fs.write"));
    assert!(!readonly.contains("fs.delete"));
}

#[tokio::test]
async fn test_pipeline_with_function_tool_call() {
    // Create a custom tool registry with a function tool
    let mut registry = ToolRegistry::new();
    
    let greet_tool = FunctionTool::new(
        "greet",
        "Greet someone",
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"]
        }),
        |args, _ctx| {
            Box::pin(async move {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("World");
                Ok(ToolOutput::text(format!("Hello, {}!", name)))
            })
        },
    );

    registry.register(greet_tool);

    // Create a pipeline that uses the tool
    let pipeline = Pipeline {
        name: "greet_pipeline".to_string(),
        steps: vec![AgentStep {
            name: "greet_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "greet".to_string(),
                args: json!({ "name": "Phase2" }),
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::Allow(vec!["greet".to_string()]),
            injection_protection: InjectionProtection::Strict,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    // Create an agent
    let agent = Agent {
        name: "test_agent".to_string(),
        description: "Test agent".to_string(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Allow(vec!["greet".to_string()]),
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
    };

    // Run with custom tool registry
    let mut runner = PipelineRunner::with_tool_registry(Arc::new(registry));
    let result = runner
        .run(&pipeline, &agent, json!({ "input": "test" }))
        .await;

    assert!(result.is_ok(), "Pipeline run should succeed");
    let pipeline_result = result.unwrap();
    assert!(pipeline_result.success, "Pipeline should report success");
    assert!(!pipeline_result.steps_passed.is_empty(), "Pipeline should have at least one passed step");
}

#[tokio::test]
async fn test_audit_log_records_pipeline_events() {
    // Create a function tool
    let mut registry = ToolRegistry::new();
    
    let test_tool = FunctionTool::new(
        "audit_test",
        "Test tool for audit",
        json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
        |_args, _ctx| {
            Box::pin(async move {
                Ok(ToolOutput::text("test output".to_string()))
            })
        },
    );

    registry.register(test_tool);

    // Create a pipeline
    let pipeline = Pipeline {
        name: "audit_pipeline".to_string(),
        steps: vec![AgentStep {
            name: "audit_step".to_string(),
            guard_in: Guard::None,
            action: StepAction::ToolCall {
                tool: "audit_test".to_string(),
                args: json!({}),
            },
            guard_out: Guard::None,
            verdict: Verdict::Automated(Guard::None),
            tools: ToolSet::Allow(vec!["audit_test".to_string()]),
            injection_protection: InjectionProtection::Strict,
            output_schema: None,
        }],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = Agent {
        name: "audit_agent".to_string(),
        description: "Audit test agent".to_string(),
        pipeline: pipeline.clone(),
        tools: ToolSet::Allow(vec!["audit_test".to_string()]),
        skills: SkillSet {
            skills: vec![],
        },
        policy: AgentPolicy::default(),
    };

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(registry));
    let result = runner
        .run(&pipeline, &agent, json!({}))
        .await;

    assert!(result.is_ok(), "Pipeline should succeed");

    // Check audit log for standard pipeline events
    let pipeline_result = result.unwrap();
    let audit_log = &pipeline_result.audit_log;
    let entries = audit_log.entries();
    
    // Verify we have pipeline start/completion events
    let has_pipeline_started = entries.iter().any(|e| {
        matches!(e.event, AuditEvent::PipelineStarted)
    });
    let has_pipeline_completed = entries.iter().any(|e| {
        matches!(e.event, AuditEvent::PipelineCompleted { .. })
    });
    let has_step_started = entries.iter().any(|e| {
        matches!(e.event, AuditEvent::StepStarted)
    });

    assert!(has_pipeline_started, "Audit log should contain PipelineStarted event");
    assert!(has_pipeline_completed, "Audit log should contain PipelineCompleted event");
    assert!(has_step_started, "Audit log should contain StepStarted event");
}
