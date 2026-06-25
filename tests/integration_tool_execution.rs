//! Integration tests: Tool execution, ToolRegistry, FunctionTool, and ToolSet scoping
//!
//! All tests run through PipelineRunner::run() with StepAction::ToolCall,
//! verifying that tool scoping is enforced end-to-end through the runner.

use std::sync::Arc;
use verdict::prelude::*;
use serde_json::json;

// â”€â”€â”€ helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

fn tool_step(name: &str, tool: &str, args: serde_json::Value, scope: ToolSet) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall { tool: tool.into(), args },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: scope,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        }
}

fn make_agent(pipeline: &Pipeline, agent_tools: ToolSet, policy_tools: ToolSet) -> Agent {
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = policy_tools;
    Agent {
        name: "test_agent".into(),
        description: "tool integration test agent".into(),
        pipeline: pipeline.clone(),
        tools: agent_tools,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    }
}

// â”€â”€â”€ Test 1: FunctionTool output becomes step output â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_function_tool_output_becomes_step_output() {
    let tool = FunctionTool::new(
        "local.produce",
        "produce a fixed value",
        json!({ "type": "object", "properties": {} }),
        |_args, _ctx| {
            Box::pin(async move { Ok(ToolOutput::text("custom-value-42".to_string())) })
        },
    );
    let mut registry = ToolRegistry::new();
    registry.register(tool);

    let scope = ToolSet::Allow(vec!["local.produce".into()]);
    let pipeline = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("produce", "local.produce", json!({}), scope.clone())],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = make_agent(&pipeline, scope.clone(), scope.clone());
    let mut runner = PipelineRunner::with_tool_registry(Arc::new(registry));
    let res = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(res.success);
    assert_eq!(res.step_results["produce"].output.raw, "custom-value-42");
    assert!(res.steps_passed.contains(&"produce".to_string()));
    assert!(res.audit_log.entries().iter().any(|e|
        matches!(&e.event, AuditEvent::ToolCallCompleted { tool, .. } if tool == "local.produce")
    ));
}

// â”€â”€â”€ Test 2: Step scope blocks tool not in Allow list â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_step_scope_allow_blocks_other_registered_tool() {
    let tool_a = FunctionTool::new("local.a", "A",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Ok(ToolOutput::text("A".to_string())) }));
    let tool_b = FunctionTool::new("local.b", "B",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Ok(ToolOutput::text("B".to_string())) }));
    let mut reg = ToolRegistry::new();
    reg.register(tool_a);
    reg.register(tool_b);
    let reg = Arc::new(reg);

    let scope = ToolSet::Allow(vec!["local.a".into()]);

    // --- success: calling local.a which is in scope ---
    let p_ok = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("call", "local.a", json!({}), scope.clone())],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent_ok = make_agent(&p_ok, scope.clone(), scope.clone());
    let ok = PipelineRunner::with_tool_registry(Arc::clone(&reg))
        .run(&p_ok, &agent_ok, json!({})).await.unwrap();
    assert_eq!(ok.step_results["call"].output.raw, "A");

    // --- blocked: calling local.b which is NOT in scope ---
    let p_bad = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("call", "local.b", json!({}), scope.clone())],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent_bad = make_agent(&p_bad, scope.clone(), scope.clone());
    let err = PipelineRunner::with_tool_registry(Arc::clone(&reg))
        .run(&p_bad, &agent_bad, json!({})).await.unwrap_err();

    match err {
        PipelineError::StepFailed { step, error } => {
            assert_eq!(step, "call");
            let msg = error.to_string();
            assert!(msg.to_lowercase().contains("not allowed") || msg.contains("local.b"),
                "error should mention 'not allowed' or tool name: {msg}");
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
}

// â”€â”€â”€ Test 3: Intersection(Allow(a,b), Allow(b,c)) â†’ only b is accessible â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_toolset_intersection_only_common_tool_accessible() {
    let mk_tool = |name: &'static str, body: &'static str| -> FunctionTool {
        FunctionTool::new(name, name,
            json!({"type":"object","properties":{}}),
            move |_a, _c| Box::pin(async move { Ok(ToolOutput::text(body.to_string())) }))
    };
    let mut reg = ToolRegistry::new();
    reg.register(mk_tool("local.a", "A"));
    reg.register(mk_tool("local.b", "B"));
    reg.register(mk_tool("local.c", "C"));
    let reg = Arc::new(reg);

    let scope = ToolSet::Intersection(
        Box::new(ToolSet::Allow(vec!["local.a".into(), "local.b".into()])),
        Box::new(ToolSet::Allow(vec!["local.b".into(), "local.c".into()])),
    );

    let run_tool = |tool: &'static str, reg: Arc<ToolRegistry>, scope: ToolSet| async move {
        let p = Pipeline {
            name: "p".into(),
            steps: vec![tool_step("s", tool, json!({}), scope.clone())],
            on_failure: FailureMode::Abort, max_retries: 0,
        };
        let agent = make_agent(&p, ToolSet::Full, ToolSet::Full);
        PipelineRunner::with_tool_registry(reg).run(&p, &agent, json!({})).await
    };

    // local.b is in both sets â†’ accessible
    let ok = run_tool("local.b", Arc::clone(&reg), scope.clone()).await.unwrap();
    assert_eq!(ok.step_results["s"].output.raw, "B");

    // local.a is only in left set â†’ blocked
    assert!(run_tool("local.a", Arc::clone(&reg), scope.clone()).await.is_err(),
        "local.a not in intersection should be blocked");

    // local.c is only in right set â†’ blocked
    assert!(run_tool("local.c", Arc::clone(&reg), scope.clone()).await.is_err(),
        "local.c not in intersection should be blocked");
}

// â”€â”€â”€ Test 4: step=Full + policy=None â†’ tool blocked â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_step_full_with_policy_none_blocks_tool() {
    let tool = FunctionTool::new("local.t", "t",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Ok(ToolOutput::text("ok".to_string())) }));
    let mut reg = ToolRegistry::new();
    reg.register(tool);

    let p = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("s", "local.t", json!({}), ToolSet::Full)],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    // policy.allowed_tools = None â†’ intersection with Full = None
    let agent = make_agent(&p, ToolSet::Full, ToolSet::None);

    let err = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap_err();

    match err {
        PipelineError::StepFailed { step, error } => {
            assert_eq!(step, "s");
            let m = error.to_string();
            assert!(m.to_lowercase().contains("not allowed") || m.contains("local.t"),
                "error should mention tool not allowed: {m}");
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
}

// â”€â”€â”€ Test 5: step=Full + policy=Full â†’ tool accessible â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_step_full_with_policy_full_allows_tool() {
    let tool = FunctionTool::new("local.t", "t",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Ok(ToolOutput::text("ok".to_string())) }));
    let mut reg = ToolRegistry::new();
    reg.register(tool);

    let p = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("s", "local.t", json!({}), ToolSet::Full)],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = make_agent(&p, ToolSet::Full, ToolSet::Full);

    let r = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap();
    assert!(r.success);
    assert_eq!(r.step_results["s"].output.raw, "ok");
    assert!(r.audit_log.entries().iter().any(|e|
        matches!(&e.event, AuditEvent::ToolCallCompleted { tool, .. } if tool == "local.t")
    ));
}

// â”€â”€â”€ Test 6: FunctionTool returning JSON â†’ output parses as JSON â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_function_tool_returns_structured_json() {
    let tool = FunctionTool::new("local.calc", "calc",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async move {
            Ok(ToolOutput::json(json!({"sum": 7, "items": ["a", "b"]})))
        }));
    let mut reg = ToolRegistry::new();
    reg.register(tool);

    let scope = ToolSet::Allow(vec!["local.calc".into()]);
    let p = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("s", "local.calc", json!({}), scope.clone())],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = make_agent(&p, scope.clone(), scope);

    let r = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap();
    assert!(r.success);

    let raw = &r.step_results["s"].output.raw;
    let v: serde_json::Value = serde_json::from_str(raw).expect("output must be valid JSON");
    assert_eq!(v["sum"], 7);
    let items = v["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0], "a");
    assert_eq!(items[1], "b");
}

// â”€â”€â”€ Test 7: ExecutionFailed error surfaces as StepFailed â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_function_tool_execution_failed_surfaces_as_step_failed() {
    let tool = FunctionTool::new("local.fail", "fail",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Err(ToolError::ExecutionFailed { reason: "boom".to_string() }) }));
    let mut reg = ToolRegistry::new();
    reg.register(tool);

    let scope = ToolSet::Allow(vec!["local.fail".into()]);
    let p = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("s", "local.fail", json!({}), scope.clone())],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = make_agent(&p, scope.clone(), scope);

    let err = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap_err();

    match err {
        PipelineError::StepFailed { step, error } => {
            assert_eq!(step, "s");
            let m = error.to_string();
            assert!(m.contains("boom"), "error should mention 'boom': {m}");
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
}

// â”€â”€â”€ Test 8: Tool registration and retrieval works end-to-end â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_arc_registered_tool_callable_via_pipeline() {
    let tool = FunctionTool::new(
        "local.arcd", "arc-registered",
        json!({"type":"object","properties":{}}),
        |_a, _c| Box::pin(async { Ok(ToolOutput::text("arcd-ok".to_string())) }),
    );
    let mut reg = ToolRegistry::new();
    reg.register(tool);

    // Verify it's retrievable before running
    assert!(reg.get("local.arcd").is_some(), "tool must be retrievable after registration");

    let scope = ToolSet::Allow(vec!["local.arcd".into()]);
    let p = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("s", "local.arcd", json!({}), scope.clone())],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = make_agent(&p, scope.clone(), scope);

    let r = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap();
    assert_eq!(r.step_results["s"].output.raw, "arcd-ok");
}

// â”€â”€â”€ Test 9: Sequential multi-tool steps each record correct output â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_multiple_sequential_tool_steps_each_record_correct_output() {
    let mk_tool = |name: &'static str, out: &'static str| FunctionTool::new(
        name, name,
        json!({"type":"object","properties":{}}),
        move |_a, _c| Box::pin(async move { Ok(ToolOutput::text(out.to_string())) }));

    let mut reg = ToolRegistry::new();
    reg.register(mk_tool("local.one", "first"));
    reg.register(mk_tool("local.two", "second"));
    reg.register(mk_tool("local.three", "third"));

    let scope = ToolSet::Allow(vec![
        "local.one".into(), "local.two".into(), "local.three".into(),
    ]);
    let p = Pipeline {
        name: "p".into(),
        steps: vec![
            tool_step("s1", "local.one",   json!({}), scope.clone()),
            tool_step("s2", "local.two",   json!({}), scope.clone()),
            tool_step("s3", "local.three", json!({}), scope.clone()),
        ],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = make_agent(&p, scope.clone(), scope);

    let r = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap();

    assert!(r.success);
    assert_eq!(r.steps_passed, vec!["s1".to_string(), "s2".to_string(), "s3".to_string()]);
    assert_eq!(r.step_results["s1"].output.raw, "first");
    assert_eq!(r.step_results["s2"].output.raw, "second");
    assert_eq!(r.step_results["s3"].output.raw, "third");

    // Verify audit log recorded all three in order
    let completed: Vec<&str> = r.audit_log.entries().iter()
        .filter_map(|e| match &e.event {
            AuditEvent::ToolCallCompleted { tool, .. } => Some(tool.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(completed, vec!["local.one", "local.two", "local.three"]);
}

// â”€â”€â”€ Test 10: ReadOnly step scope blocks fs.write â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_readonly_step_scope_blocks_fs_write() {
    let reg = ToolRegistry::with_builtins();
    let target = std::env::temp_dir().join("verdict_readonly_block_test.txt");
    let _ = std::fs::remove_file(&target);

    let p = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("w", "fs.write",
            json!({ "path": target.to_string_lossy(), "content": "should not appear" }),
            ToolSet::ReadOnly)],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    // Agent has Full tools but step only has ReadOnly
    let agent = make_agent(&p, ToolSet::Full, ToolSet::Full);

    let err = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap_err();

    match err {
        PipelineError::StepFailed { step, error } => {
            assert_eq!(step, "w");
            let m = error.to_string();
            assert!(m.to_lowercase().contains("not allowed") || m.contains("fs.write"),
                "error should mention 'not allowed': {m}");
        }
        other => panic!("expected StepFailed, got {other:?}"),
    }
    assert!(!target.exists(), "fs.write must not have created the target file");
}

// â”€â”€â”€ Test 11: Built-in fs.read reads Cargo.toml â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_builtin_fs_read_reads_cargo_toml_in_pipeline() {
    let reg = ToolRegistry::with_builtins();
    let path = std::env::current_dir().unwrap().join("Cargo.toml");
    assert!(path.exists(), "precondition: Cargo.toml must exist at {path:?}");

    let p = Pipeline {
        name: "p".into(),
        steps: vec![tool_step("read", "fs.read",
            json!({ "path": path.to_string_lossy() }),
            ToolSet::ReadOnly)],
        on_failure: FailureMode::Abort, max_retries: 0,
    };
    let agent = make_agent(&p, ToolSet::ReadOnly, ToolSet::ReadOnly);

    let r = PipelineRunner::with_tool_registry(Arc::new(reg))
        .run(&p, &agent, json!({})).await.unwrap();

    assert!(r.success);
    let raw = &r.step_results["read"].output.raw;
    assert!(raw.contains("[package]"), "Cargo.toml must contain [package]: {raw}");
    assert!(raw.contains("verdict"), "Cargo.toml must contain 'verdict': {raw}");
    assert!(r.audit_log.entries().iter().any(|e|
        matches!(&e.event, AuditEvent::ToolCallCompleted { tool, .. } if tool == "fs.read")
    ));
}

// â”€â”€â”€ Test 12: FunctionTool echoes args back and validates schema â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[tokio::test]
async fn test_function_tool_echoes_args_and_validates_schema() {
    let tool = FunctionTool::new("local.echo", "echo input",
        json!({
            "type": "object",
            "required": ["value"],
            "properties": { "value": { "type": "string" } }
        }),
        |args, _c| Box::pin(async move {
            let v = args.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
            Ok(ToolOutput::text(v))
        }));
    let mut reg = ToolRegistry::new();
    reg.register(tool);
    let reg = Arc::new(reg);
    let scope = ToolSet::Allow(vec!["local.echo".into()]);

    let run_with = |args: serde_json::Value, reg: Arc<ToolRegistry>, scope: ToolSet| async move {
        let p = Pipeline {
            name: "p".into(),
            steps: vec![tool_step("s", "local.echo", args, scope.clone())],
            on_failure: FailureMode::Abort, max_retries: 0,
        };
        let agent = make_agent(&p, scope.clone(), scope);
        PipelineRunner::with_tool_registry(reg).run(&p, &agent, json!({})).await
    };

    // Happy path: correct args
    let r = run_with(json!({"value": "hello-world"}), Arc::clone(&reg), scope.clone()).await.unwrap();
    assert_eq!(r.step_results["s"].output.raw, "hello-world");

    // Missing required field â†’ schema validation fails
    let err = run_with(json!({}), Arc::clone(&reg), scope.clone()).await.unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.to_lowercase().contains("schema") || msg.to_lowercase().contains("invalid") || msg.contains("StepFailed"),
        "missing arg should produce a schema/validation error: {msg}");
}

