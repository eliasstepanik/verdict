//! Phase 16: Dynamic Prompt Templates and Structured Output
//!
//! Tests for composable prompt templates, structured output types, and related guards.

use serde_json::json;
use std::path::PathBuf;
use verdict::prelude::*;

#[tokio::test]
async fn test_prompt_template_static() {
    let template = PromptTemplate::new().push_static("Hello, world!");
    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );
    let result = template.render(&ctx).await.unwrap();
    assert_eq!(result, "Hello, world!");
}

#[tokio::test]
async fn test_prompt_template_conversation() {
    use verdict::llm::provider::{ChatMessage, ChatRole, MessageHistory};

    let template = PromptTemplate::new()
        .push_static("Messages:\n")
        .push_conversation(2);

    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    ctx.conversation_history = MessageHistory {
        conversation_id: None,
        messages: vec![
            ChatMessage {
                role: ChatRole::User,
                content: "First message".to_string(),
                tool_calls_json: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: ChatRole::Assistant,
                content: "First response".to_string(),
                tool_calls_json: None,
                tool_call_id: None,
            },
            ChatMessage {
                role: ChatRole::User,
                content: "Second message".to_string(),
                tool_calls_json: None,
                tool_call_id: None,
            },
        ],
    };

    let result = template.render(&ctx).await.unwrap();
    assert!(result.contains("Messages:"));
    assert!(result.contains("First response"));
    assert!(result.contains("Second message"));
}

#[tokio::test]
async fn test_prompt_template_multiple_segments() {
    let template = PromptTemplate::new()
        .push_static("Start: ")
        .push_static("middle ")
        .push_static("end");

    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    let result = template.render(&ctx).await.unwrap();
    assert_eq!(result, "Start: middle end");
}

#[tokio::test]
async fn test_structured_output_code() {
    let code_output = StructuredOutput::Code {
        language: "rust".to_string(),
        source: "fn main() {}".to_string(),
    };

    let tool_output = ToolOutput::with_structured("fn main() {}".to_string(), code_output.clone());
    assert_eq!(tool_output.raw, "fn main() {}");
    assert!(tool_output.as_structured().is_some());

    match tool_output.as_structured().unwrap() {
        StructuredOutput::Code { language, source } => {
            assert_eq!(language, "rust");
            assert_eq!(source, "fn main() {}");
        }
        _ => panic!("Wrong structured output type"),
    }
}

#[tokio::test]
async fn test_structured_output_diagnostics() {
    let diagnostics = vec![
        DiagnosticEntry {
            severity: DiagnosticSeverity::Error,
            message: "undefined variable".to_string(),
            file: Some("main.rs".to_string()),
            line: Some(5),
        },
        DiagnosticEntry {
            severity: DiagnosticSeverity::Warning,
            message: "unused import".to_string(),
            file: Some("lib.rs".to_string()),
            line: Some(2),
        },
    ];

    let output = ToolOutput::with_structured(
        "error: undefined variable".to_string(),
        StructuredOutput::Diagnostics(diagnostics),
    );

    match output.as_structured().unwrap() {
        StructuredOutput::Diagnostics(diags) => {
            assert_eq!(diags.len(), 2);
            assert_eq!(diags[0].severity, DiagnosticSeverity::Error);
            assert_eq!(diags[1].severity, DiagnosticSeverity::Warning);
        }
        _ => panic!("Wrong structured output type"),
    }
}

#[tokio::test]
async fn test_structured_output_table() {
    let table = StructuredOutput::Table {
        headers: vec!["Name".to_string(), "Age".to_string()],
        rows: vec![
            vec!["Alice".to_string(), "30".to_string()],
            vec!["Bob".to_string(), "25".to_string()],
        ],
    };

    let output = ToolOutput::with_structured("Name | Age\nAlice | 30\nBob | 25".to_string(), table);

    match output.as_structured().unwrap() {
        StructuredOutput::Table { headers, rows } => {
            assert_eq!(headers.len(), 2);
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0][0], "Alice");
        }
        _ => panic!("Wrong structured output type"),
    }
}

#[tokio::test]
async fn test_guard_structured_output_present_with_output() {
    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    // Set output with parsed JSON (indicating structured data)
    ctx.output = Some(StepOutput {
        raw: r#"{"status": "success"}"#.to_string(),
        parsed: Some(json!({"status": "success"})),
        eval_result: None,
    });

    let guard = Guard::StructuredOutputPresent;
    let result = GuardEngine::evaluate(&guard, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_guard_structured_output_missing() {
    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    // Set output without parsed JSON
    ctx.output = Some(StepOutput {
        raw: "plain text output".to_string(),
        parsed: None,
        eval_result: None,
    });

    let guard = Guard::StructuredOutputPresent;
    let result = GuardEngine::evaluate(&guard, &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_guard_structured_output_no_output() {
    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    // No output set
    let guard = Guard::StructuredOutputPresent;
    let result = GuardEngine::evaluate(&guard, &ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_guard_prompt_template_rendered() {
    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    // This guard always passes — rendering failure would occur at action execution time
    let guard = Guard::PromptTemplateRendered;
    let result = GuardEngine::evaluate(&guard, &ctx).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_prompt_template_step_output() {
    let template = PromptTemplate::new()
        .push_static("Previous step output: ")
        .push_step_output("prior_step");

    let mut ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    // Add a prior step result
    let prior_result = StepResult {
        step_name: "prior_step".to_string(),
        output: StepOutput {
            raw: "test output".to_string(),
            parsed: None,
            eval_result: None,
        },
        verdict_passed: true,
        error: None,
    };
    ctx.step_results
        .insert("prior_step".to_string(), prior_result);

    let result = template.render(&ctx).await.unwrap();
    assert_eq!(result, "Previous step output: test output");
}

#[tokio::test]
async fn test_prompt_template_missing_step() {
    let template = PromptTemplate::new().push_step_output("nonexistent_step");

    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    let result = template.render(&ctx).await;
    assert!(matches!(result, Err(PromptError::StepNotFound(_))));
}

#[tokio::test]
async fn test_structured_output_file_snippet() {
    let snippet = StructuredOutput::FileSnippet {
        path: "src/main.rs".to_string(),
        start_line: 10,
        end_line: 20,
        content: "fn main() {\n    println!(\"Hello\");\n}".to_string(),
        language: Some("rust".to_string()),
    };

    let output = ToolOutput::with_structured(
        "fn main() {\n    println!(\"Hello\");\n}".to_string(),
        snippet,
    );

    match output.as_structured().unwrap() {
        StructuredOutput::FileSnippet {
            path,
            start_line,
            end_line,
            content,
            language,
        } => {
            assert_eq!(path, "src/main.rs");
            assert_eq!(*start_line, 10);
            assert_eq!(*end_line, 20);
            assert!(content.contains("main"));
            assert_eq!(language, &Some("rust".to_string()));
        }
        _ => panic!("Wrong structured output type"),
    }
}

#[tokio::test]
async fn test_structured_output_custom() {
    let custom_value = json!({
        "custom_field": "custom_value",
        "nested": {
            "data": 42
        }
    });

    let custom = StructuredOutput::Custom {
        schema: "MyCustomSchema".to_string(),
        value: custom_value.clone(),
    };

    let output = ToolOutput::with_structured(custom_value.to_string(), custom);

    match output.as_structured().unwrap() {
        StructuredOutput::Custom { schema, value } => {
            assert_eq!(schema, "MyCustomSchema");
            assert_eq!(value["custom_field"].as_str().unwrap(), "custom_value");
            assert_eq!(value["nested"]["data"].as_i64().unwrap(), 42);
        }
        _ => panic!("Wrong structured output type"),
    }
}

#[tokio::test]
async fn test_tool_output_backwards_compatibility() {
    // Ensure that old-style ToolOutput construction still works
    let output1 = ToolOutput::text("plain text".to_string());
    assert_eq!(output1.raw, "plain text");
    assert!(output1.parsed.is_none());
    assert!(output1.as_structured().is_none());

    let json_val = json!({"key": "value"});
    let output2 = ToolOutput::json(json_val.clone());
    assert!(output2.parsed.is_some());
    assert_eq!(output2.parsed.as_ref().unwrap(), &json_val);
    assert!(output2.as_structured().is_none());
}

#[tokio::test]
async fn test_prompt_template_empty() {
    let template = PromptTemplate::new();
    let ctx = StepContext::new(
        "test_agent".to_string(),
        "test_pipeline".to_string(),
        "test_step".to_string(),
        json!({}),
        FilesystemPolicy {
            workspace_root: PathBuf::from("."),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        },
    );

    let result = template.render(&ctx).await.unwrap();
    assert_eq!(result, "");
}

#[tokio::test]
async fn test_prompt_template_default() {
    let template = PromptTemplate::default();
    assert!(template.segments.is_empty());
}

#[test]
fn test_diagnostic_severity_equality() {
    assert_eq!(DiagnosticSeverity::Error, DiagnosticSeverity::Error);
    assert_ne!(DiagnosticSeverity::Error, DiagnosticSeverity::Warning);
}
