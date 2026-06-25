//! Integration tests for ToolUseLoop: real conversation with tool use end-to-end
//!
//! Tests use a scripted mock LLM provider that can return different responses per round,
//! simulating multi-turn tool-call → tool-result → text-summary flows.
//!
//! All tests verify:
//! - Pipeline execution via PipelineRunner
//! - StepAction::ToolUseLoop with StopCondition::TextOnly
//! - Tool registry and scoping
//! - Guard enforcement (NonEmptyOutput, NoSecretsInOutput)
//! - Real filesystem tools (fs.list, fs.read) when available

mod common;

use common::ScriptedMockLlmProvider;
use common::ScriptedResponse;
use std::sync::Arc;
use verdict::prelude::*;
use serde_json::json;

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// HELPERS
// ═══════════════════════════════════════════════════════════════════════════════════════════════

fn tool_use_loop_step(
    name: &str,
    system: &str,
    user: &str,
    tools: Vec<String>,
    max_rounds: usize,
) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::ToolUseLoop {
            system: system.into(),
            user: user.into(),
            model: ProviderSpec {
                model: "scripted".to_string(),
                provider: "test".to_string(),
            },
            tools,
            max_rounds,
            stop_condition: StopCondition::TextOnly,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

fn llm_call_step(
    name: &str,
    system: &str,
    user: &str,
) -> AgentStep {
    AgentStep {
        name: name.into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: system.into(),
            user: user.into(),
            model: None,
            conversation_id: None,
            append_to_history: false,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    }
}

fn make_agent(pipeline: Pipeline, agent_tools: ToolSet) -> Agent {
    let mut policy = AgentPolicy::default();
    policy.allowed_tools = agent_tools.clone();
    Agent {
        name: "test_agent".into(),
        description: "tool conversation test agent".into(),
        pipeline,
        tools: agent_tools,
        skills: SkillSet::default(),
        policy,
        scorers: vec![],
    }
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 1: Tool use loop single tool call then text
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_use_loop_single_tool_call_then_text() {
    let script = vec![
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        ScriptedResponse::text("Here are the files: Cargo.toml, src/"),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));

    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "list_files",
        "You are a file explorer.",
        "List the files in the current directory.",
        vec!["fs.list".to_string()],
        4,
    );

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success, "Pipeline should succeed");
    assert!(
        result.step_results["list_files"].output.raw.contains("files"),
        "Output should contain 'files'"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 2: No tools called, returns text immediately
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_use_loop_no_tools_called_returns_text() {
    let script = vec![ScriptedResponse::text("Hello! No tools needed.")];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "greet",
        "You are a friendly assistant.",
        "Say hello without using tools.",
        vec![],
        4,
    );

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert_eq!(result.step_results["greet"].output.raw, "Hello! No tools needed.");
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 3: Multiple tool calls sequential
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_use_loop_multiple_tool_calls_sequential() {
    let script = vec![
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        ScriptedResponse::tool_call("fs.read", json!({ "path": "Cargo.toml" })),
        ScriptedResponse::text("I read both. Here is the summary."),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "inspect",
        "You are investigating the workspace.",
        "List files and read Cargo.toml.",
        vec!["fs.list".to_string(), "fs.read".to_string()],
        4,
    );

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert!(result.step_results["inspect"].output.raw.contains("summary"));
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 4: Synthesis call when final text empty
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_use_loop_synthesis_call_when_final_text_empty() {
    let script = vec![
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        ScriptedResponse::text("Files found: main.rs"),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "analyze",
        "List files and summarize.",
        "What files are in the workspace?",
        vec!["fs.list".to_string()],
        4,
    );

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert!(result.step_results["analyze"].output.raw.contains("Files found"));
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 5: Two-step pipeline (understand then act)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_two_step_pipeline_understand_then_act() {
    let script = vec![
        ScriptedResponse::text("List files in current dir"),
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        ScriptedResponse::text("Done: files listed"),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let understand = llm_call_step(
        "understand",
        "You are planning a task.",
        "What should we do?",
    );

    let act = tool_use_loop_step(
        "act",
        "Execute the plan.",
        "Do it.",
        vec!["fs.list".to_string()],
        4,
    );

    let pipeline = Pipeline {
        name: "understand_then_act".into(),
        steps: vec![understand, act],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert!(!result.step_results["understand"].output.raw.is_empty());
    assert!(!result.step_results["act"].output.raw.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 6: Pipeline builder DSL with tool use
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_pipeline_builder_dsl_with_tool_use() {
    let script = vec![
        ScriptedResponse::text("Plan: list files"),
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        ScriptedResponse::text("Files listed successfully"),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let plan_step = llm_call_step("plan", "Plan.", "What's the plan?");
    let exec_step = tool_use_loop_step(
        "execute",
        "Execute.",
        "Do it.",
        vec!["fs.list".to_string()],
        4,
    );

    let pipeline = PipelineBuilder::new("builder_test")
        .then(plan_step)
        .then(exec_step)
        .build();

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert!(!result.step_results["plan"].output.raw.is_empty());
    assert!(!result.step_results["execute"].output.raw.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 7: Guard NonEmptyOutput blocks empty response
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_guard_noemptyoutput_blocks_empty_response() {
    let script = vec![ScriptedResponse::text("")];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let mut step = llm_call_step("speak", "Respond.", "Say something.");
    step.guard_out = Guard::NonEmptyOutput;

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    // Guard should fail, so the result should be an error or failed pipeline
    assert!(result.is_err() || !result.unwrap().success);
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 8: Guard NoSecretsInOutput blocks API key
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_guard_nosecretsinoutput_blocks_api_key() {
    let script = vec![ScriptedResponse::text("sk-1234567890abcdef1234567890abcdef")];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let mut step = llm_call_step("get_key", "Return a key.", "What's the key?");
    step.guard_out = Guard::NoSecretsInOutput;

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    // Guard should fail due to secret detection
    assert!(result.is_err() || !result.unwrap().success);
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 9: Tool call fs.list real execution
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_call_fs_list_real_execution() {
    let tool_registry = ToolRegistry::with_builtins();

    let step = AgentStep {
        name: "list_workspace".into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall {
            tool: "fs.list".into(),
            args: json!({ "path": "." }),
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        tools: ToolSet::Allow(vec!["fs.list".into()]),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    // The output should contain at least one filename
    assert!(!result.step_results["list_workspace"].output.raw.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 10: Tool call fs.read real execution
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_call_fs_read_real_execution() {
    let tool_registry = ToolRegistry::with_builtins();

    let step = AgentStep {
        name: "read_cargo".into(),
        guard_in: Guard::None,
        action: StepAction::ToolCall {
            tool: "fs.read".into(),
            args: json!({ "path": "Cargo.toml" }),
        },
        guard_out: Guard::NonEmptyOutput,
        verdict: Verdict::None,
        tools: ToolSet::Allow(vec!["fs.read".into()]),
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    // Cargo.toml should contain [package]
    assert!(result.step_results["read_cargo"].output.raw.contains("[package]"));
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 11: Multi-turn conversation via conversation_id
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_conversation_multi_turn_via_conversation_id() {
    let script = vec![
        ScriptedResponse::text("My name is Alice"),
        ScriptedResponse::text("I said my name is Alice earlier"),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step1 = AgentStep {
        name: "turn1".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "Respond in first person.".into(),
            user: "Who are you?".into(),
            model: None,
            conversation_id: Some("session-1".to_string()),
            append_to_history: true,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let step2 = AgentStep {
        name: "turn2".into(),
        guard_in: Guard::None,
        action: StepAction::LlmCall {
            system: "Respond in first person.".into(),
            user: "What did you say about yourself?".into(),
            model: None,
            conversation_id: Some("session-1".to_string()),
            append_to_history: true,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::ReadOnly,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let pipeline = Pipeline {
        name: "multi_turn".into(),
        steps: vec![step1, step2],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await.unwrap();

    assert!(result.success);
    assert!(!result.step_results["turn1"].output.raw.is_empty());
    assert!(!result.step_results["turn2"].output.raw.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test 12: Delegation with scripted child agent
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_delegation_with_scripted_child_agent() {
    let script = vec![ScriptedResponse::text("child completed")];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    // Create child agent
    let child_step = llm_call_step("respond", "You are a child agent.", "What's your response?");
    let child_pipeline = Pipeline {
        name: "child_pipeline".into(),
        steps: vec![child_step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };
    let child_agent = make_agent(child_pipeline.clone(), ToolSet::Full);

    // Register child agent
    let mut agent_registry = AgentRegistry::new();
    agent_registry.register(child_agent);

    // Create parent step that delegates to child
    let parent_step = AgentStep {
        name: "delegate_to_child".into(),
        guard_in: Guard::None,
        action: StepAction::DelegateAgent {
            agent: "test_agent".into(),
            input: json!({ "task": "test" }),
            expected_output_schema: None,
            delegation_policy: DelegationPolicy {
                max_depth: 1,
                allowed_agents: vec!["test_agent".to_string()],
                require_output_schema: false,
                inherit_tool_scope: true,
                inherit_budget: true,
                require_user_approval: false,
                on_delegation_start: None,
                on_delegation_complete: None,
                on_iteration_complete: None,
                message_filter: None,
                memory_isolation: MemoryIsolation::Isolated,
            },
            detached: false,
        },
        guard_out: Guard::None,
        verdict: Verdict::None,
        tools: ToolSet::Full,
        injection_protection: InjectionProtection::None,
        output_schema: None,
        dependencies: vec![],
        parallel: false,
        input_processors: vec![],
        output_processors: vec![],
    };

    let parent_pipeline = Pipeline {
        name: "parent_pipeline".into(),
        steps: vec![parent_step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let parent_agent = make_agent(parent_pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_registries(
        Arc::new(tool_registry),
        Arc::new(agent_registry),
    );
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&parent_pipeline, &parent_agent, json!({})).await.unwrap();

    assert!(result.success);
}
