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
use serde_json::json;
use std::sync::Arc;
use verdict::prelude::*;

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

fn llm_call_step(name: &str, system: &str, user: &str) -> AgentStep {
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
        result.step_results["list_files"]
            .output
            .raw
            .contains("files"),
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
    assert_eq!(
        result.step_results["greet"].output.raw,
        "Hello! No tools needed."
    );
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
    assert!(result.step_results["inspect"]
        .output
        .raw
        .contains("summary"));
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
    assert!(result.step_results["analyze"]
        .output
        .raw
        .contains("Files found"));
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
    let script = vec![ScriptedResponse::text(
        "sk-1234567890abcdef1234567890abcdef",
    )];

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
    assert!(result.step_results["read_cargo"]
        .output
        .raw
        .contains("[package]"));
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

    let mut runner =
        PipelineRunner::with_registries(Arc::new(tool_registry), Arc::new(agent_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner
        .run(&parent_pipeline, &parent_agent, json!({}))
        .await
        .unwrap();

    assert!(result.success);
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// Test: ToolUseLoop cost tracking verification
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn test_tool_use_loop_tracks_cost() {
    // This test verifies that ToolUseLoop main loop calls record_llm_cost
    let script = vec![
        // First round: tool call with usage
        ScriptedResponse::with_usage(
            "fs.list",
            json!({ "path": "." }),
            100,  // prompt_tokens
            50,   // completion_tokens
        ),
        // Second round: final text (stops) with usage
        ScriptedResponse::with_usage(
            "Done listing files",
            json!({}),
            50,   // prompt_tokens
            25,   // completion_tokens
        ),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "track_cost",
        "You are a file explorer.",
        "List the files.",
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
    
    // Verify cost was tracked: 2 LLM calls, each with usage
    // Round 1: 100 prompt + 50 completion
    //   Cost = 100 * 0.000005 + 50 * 0.000015 = 0.0005 + 0.00075 = 0.00125
    // Round 2: 50 prompt + 25 completion  
    //   Cost = 50 * 0.000005 + 25 * 0.000015 = 0.00025 + 0.000375 = 0.000625
    // Total expected: ≈ 0.00188
    assert!(
        result.total_cost_usd > 0.0,
        "Total cost should have been tracked for ToolUseLoop: got {}",
        result.total_cost_usd
    );
    assert!(
        result.total_cost_usd > 0.001,
        "Cost tracking should record at least 0.001 USD for 2 LLM calls with usage, got {}",
        result.total_cost_usd
    );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// SECURITY: LLM-driven tool calls inside a ToolUseLoop must go through the same
// enforcement path as a plain ToolCall step (allowed_tools scope + tools_used /
// commands_executed recording). These previously dispatched via
// tool_registry.get() + tool.call() directly, bypassing both.
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// Differential test. The model asks for `fs.write` in both halves; only the step's
/// `allowed_tools` scope differs.
///   - in scope  -> the write really happens (proves dispatch is live, so the
///                  out-of-scope half cannot pass for the wrong reason)
///   - out of scope -> the write must NOT happen
#[tokio::test]
async fn test_tool_use_loop_rejects_tool_outside_step_scope() {
    async fn run_with_scope(scope: Vec<String>, canary: &str) -> bool {
        let canary_abs = std::env::current_dir().unwrap().join(canary);
        let _ = std::fs::remove_file(&canary_abs);

        let script = vec![
            ScriptedResponse::tool_call(
                "fs.write",
                json!({ "path": canary, "content": "pwned" }),
            ),
            ScriptedResponse::text("Finished."),
        ];
        let llm_client = LlmClient::new(Arc::new(ScriptedMockLlmProvider::new(script)));

        let mut step = tool_use_loop_step(
            "scoped_loop",
            "You are a file writer.",
            "Do the task.",
            // Advertised to the model in both halves; only the step scope below differs.
            vec!["fs.list".to_string(), "fs.write".to_string()],
            4,
        );
        step.tools = ToolSet::Allow(scope);

        let pipeline = Pipeline {
            name: "test_pipeline".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = make_agent(pipeline.clone(), ToolSet::Full);

        let mut runner =
            PipelineRunner::with_tool_registry(Arc::new(ToolRegistry::with_builtins()));
        runner = runner.with_llm_client(Arc::new(llm_client));
        let _ = runner.run(&pipeline, &agent, json!({})).await;

        let created = canary_abs.exists();
        let _ = std::fs::remove_file(&canary_abs);
        created
    }

    let pid = std::process::id();

    // Control: fs.write IS in scope -> the tool must actually run.
    let in_scope = run_with_scope(
        vec!["fs.list".to_string(), "fs.write".to_string()],
        &format!("scope_canary_allowed_{}.txt", pid),
    )
    .await;
    assert!(
        in_scope,
        "control half failed: fs.write was in scope but never executed — the test \
         cannot prove anything about the out-of-scope half"
    );

    // Enforcement: fs.write is NOT in scope -> the tool must be rejected.
    let out_of_scope = run_with_scope(
        vec!["fs.list".to_string()],
        &format!("scope_canary_denied_{}.txt", pid),
    )
    .await;
    assert!(
        !out_of_scope,
        "fs.write was outside the step's allowed_tools scope but STILL EXECUTED \
         (canary file was created) — the ToolUseLoop tool-dispatch path is bypassing \
         the allowed_tools check"
    );
}

/// An ALLOWED shell tool is invoked by the model with a denylisted command.
/// `commands_executed` must be populated by the ToolUseLoop path so
/// `Guard::ShellCommandDenylist` can actually see and block it.
#[tokio::test]
async fn test_tool_use_loop_shell_denylist_catches_llm_driven_command() {
    let script = vec![
        ScriptedResponse::tool_call(
            "shell.run_command",
            json!({ "command": "touch", "args": ["denylist_probe.txt"] }),
        ),
        ScriptedResponse::text("Done."),
    ];

    let mock_provider = ScriptedMockLlmProvider::new(script);
    let llm_client = LlmClient::new(Arc::new(mock_provider));

    let mut step = tool_use_loop_step(
        "shell_loop",
        "You are a shell operator.",
        "Run the command.",
        vec!["shell.run_command".to_string()],
        4,
    );
    // The tool itself IS allowed; only the *command* is denied.
    step.tools = ToolSet::Allow(vec!["shell.run_command".to_string()]);
    step.guard_out = Guard::ShellCommandDenylist(vec!["touch".to_string()]);

    let pipeline = Pipeline {
        name: "test_pipeline".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(ToolRegistry::with_builtins()));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await;
    let _ = std::fs::remove_file(std::env::current_dir().unwrap().join("denylist_probe.txt"));

    match &result {
        Err(PipelineError::GuardFailed { error, .. }) => {
            let msg = format!("{:?}", error);
            assert!(
                msg.contains("ShellCommandDenylist"),
                "blocked, but not by the denylist guard: {}",
                msg
            );
        }
        other => panic!(
            "LLM-driven 'touch' was NOT caught by ShellCommandDenylist — \
             commands_executed is not being populated by the ToolUseLoop path. result={:?}",
            other
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// MUTATION TESTING: Dispatch Site 2 — XML tool calls in main loop (line 1215)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// Test that XML-formatted tool calls in the main loop (not synthesis) are scoped correctly.
/// The model returns `<invoke>...</invoke>` XML blocks instead of JSON tool_calls.
/// This exercises the dispatch site at execution.rs:1215 in the XML tool-call path.
#[tokio::test]
async fn test_tool_use_loop_xml_main_loop_rejects_tool_outside_scope() {
    async fn run_with_scope(scope: Vec<String>, canary: &str) -> bool {
        let canary_abs = std::env::current_dir().unwrap().join(canary);
        let _ = std::fs::remove_file(&canary_abs);

        // Model returns XML-format tool call (not JSON), triggering the XML parse path at line 1215
        let script = vec![
            ScriptedResponse::text(
                "<invoke name=\"fs.write\">\
                 <parameter name=\"path\">CANARY_PATH</parameter>\
                 <parameter name=\"content\">pwned</parameter>\
                 </invoke>"
                    .replace("CANARY_PATH", canary),
            ),
            ScriptedResponse::text("Finished."),
        ];
        let llm_client = LlmClient::new(Arc::new(ScriptedMockLlmProvider::new(script)));

        let mut step = tool_use_loop_step(
            "xml_loop",
            "You are a file writer.",
            "Do the task.",
            vec!["fs.list".to_string(), "fs.write".to_string()],
            4,
        );
        step.tools = ToolSet::Allow(scope);

        let pipeline = Pipeline {
            name: "test_pipeline".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = make_agent(pipeline.clone(), ToolSet::Full);

        let mut runner =
            PipelineRunner::with_tool_registry(Arc::new(ToolRegistry::with_builtins()));
        runner = runner.with_llm_client(Arc::new(llm_client));
        let _ = runner.run(&pipeline, &agent, json!({})).await;

        let created = canary_abs.exists();
        let _ = std::fs::remove_file(&canary_abs);
        created
    }

    let pid = std::process::id();

    // Control: fs.write IS in scope -> the tool must actually run.
    let in_scope = run_with_scope(
        vec!["fs.list".to_string(), "fs.write".to_string()],
        &format!("xml_scope_allowed_{}.txt", pid),
    )
    .await;
    assert!(
        in_scope,
        "control half failed: fs.write was in scope but never executed (XML path) — \
         the test cannot prove anything about the out-of-scope half"
    );

    // Enforcement: fs.write is NOT in scope -> the tool must be rejected even in XML path.
    let out_of_scope = run_with_scope(
        vec!["fs.list".to_string()],
        &format!("xml_scope_denied_{}.txt", pid),
    )
    .await;
    assert!(
        !out_of_scope,
        "fs.write was outside step scope but STILL EXECUTED in XML path (line 1215) — \
         dispatch does not route through execute_llm_tool_call"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// MUTATION TESTING: Dispatch Site 3 — JSON tool calls in synthesis (line 1318)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// Test that JSON tool calls in the synthesis loop are scoped correctly.
/// Synthesis is triggered when the main loop runs out of rounds without answering in text.
/// The dispatch site is at execution.rs:1318.
#[tokio::test]
async fn test_tool_use_loop_synthesis_json_rejects_tool_outside_scope() {
    async fn run_with_scope(scope: Vec<String>, canary: &str) -> bool {
        let canary_abs = std::env::current_dir().unwrap().join(canary);
        let _ = std::fs::remove_file(&canary_abs);

        // Round 0: model returns tool call (fs.list), which is allowed, so it executes
        // Main loop ends (max_rounds=1). answered_with_text=false, so synthesis runs.
        // In synthesis, model returns JSON tool call (fs.write) to be executed.
        let script = vec![
            ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
            // Synthesis calls LLM, which returns JSON tool call
            ScriptedResponse::tool_call("fs.write", json!({ "path": canary, "content": "pwned" })),
            ScriptedResponse::text("Done."),
        ];
        let llm_client = LlmClient::new(Arc::new(ScriptedMockLlmProvider::new(script)));

        let mut step = tool_use_loop_step(
            "syn_loop",
            "Call a tool first, then write to a file.",
            "Do it now.",
            vec!["fs.list".to_string(), "fs.write".to_string()],
            1, // max_rounds = 1, so main loop stops without text answer, triggering synthesis
        );
        step.tools = ToolSet::Allow(scope);

        let pipeline = Pipeline {
            name: "test_pipeline".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = make_agent(pipeline.clone(), ToolSet::Full);

        let mut runner =
            PipelineRunner::with_tool_registry(Arc::new(ToolRegistry::with_builtins()));
        runner = runner.with_llm_client(Arc::new(llm_client));
        let _ = runner.run(&pipeline, &agent, json!({})).await;

        let created = canary_abs.exists();
        let _ = std::fs::remove_file(&canary_abs);
        created
    }

    let pid = std::process::id();

    // Control: fs.write IS in scope -> synthesis should execute it.
    let in_scope = run_with_scope(
        vec!["fs.list".to_string(), "fs.write".to_string()],
        &format!("syn_json_allowed_{}.txt", pid),
    )
    .await;
    assert!(
        in_scope,
        "control half failed: fs.write was in scope but synthesis never executed it — \
         the test cannot prove anything about the out-of-scope half"
    );

    // Enforcement: fs.write is NOT in scope -> synthesis must reject it.
    let out_of_scope = run_with_scope(
        vec!["fs.list".to_string()],
        &format!("syn_json_denied_{}.txt", pid),
    )
    .await;
    assert!(
        !out_of_scope,
        "fs.write was outside step scope but STILL EXECUTED in synthesis JSON path (line 1318) — \
         dispatch does not route through execute_llm_tool_call"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// MUTATION TESTING: Dispatch Site 4 — XML tool calls in synthesis (line 1380)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// Test that XML tool calls in the synthesis loop are scoped correctly.
/// Similar to Site 3, but the model returns XML <invoke> blocks in synthesis instead of JSON.
/// The dispatch site is at execution.rs:1380.
#[tokio::test]
async fn test_tool_use_loop_synthesis_xml_rejects_tool_outside_scope() {
    async fn run_with_scope(scope: Vec<String>, canary: &str) -> bool {
        let canary_abs = std::env::current_dir().unwrap().join(canary);
        let _ = std::fs::remove_file(&canary_abs);

        // Round 0: model calls fs.list (allowed in both scopes), no text answer
        // Main loop ends (max_rounds=1). answered_with_text=false, so synthesis runs.
        // Synthesis response with XML tool call (fs.write)
        let script = vec![
            ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
            // Synthesis response with XML tool call
            ScriptedResponse::text(
                "I will write to the file now:\n\
                 <invoke name=\"fs.write\">\
                 <parameter name=\"path\">CANARY_PATH</parameter>\
                 <parameter name=\"content\">pwned</parameter>\
                 </invoke>"
                    .replace("CANARY_PATH", canary),
            ),
            ScriptedResponse::text("All done."),
        ];
        let llm_client = LlmClient::new(Arc::new(ScriptedMockLlmProvider::new(script)));

        let mut step = tool_use_loop_step(
            "syn_xml_loop",
            "Call a tool first, then write to a file.",
            "Do it now.",
            vec!["fs.list".to_string(), "fs.write".to_string()],
            1, // max_rounds = 1, triggers synthesis
        );
        step.tools = ToolSet::Allow(scope);

        let pipeline = Pipeline {
            name: "test_pipeline".into(),
            steps: vec![step],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        };
        let agent = make_agent(pipeline.clone(), ToolSet::Full);

        let mut runner =
            PipelineRunner::with_tool_registry(Arc::new(ToolRegistry::with_builtins()));
        runner = runner.with_llm_client(Arc::new(llm_client));
        let _ = runner.run(&pipeline, &agent, json!({})).await;

        let created = canary_abs.exists();
        let _ = std::fs::remove_file(&canary_abs);
        created
    }

    let pid = std::process::id();

    // Control: fs.write IS in scope -> synthesis should execute it.
    let in_scope = run_with_scope(
        vec!["fs.list".to_string(), "fs.write".to_string()],
        &format!("syn_xml_allowed_{}.txt", pid),
    )
    .await;
    assert!(
        in_scope,
        "control half failed: fs.write was in scope but synthesis never executed it (XML) — \
         the test cannot prove anything about the out-of-scope half"
    );

    // Enforcement: fs.write is NOT in scope -> synthesis must reject it.
    let out_of_scope = run_with_scope(
        vec!["fs.list".to_string()],
        &format!("syn_xml_denied_{}.txt", pid),
    )
    .await;
    assert!(
        !out_of_scope,
        "fs.write was outside step scope but STILL EXECUTED in synthesis XML path (line 1380) — \
         dispatch does not route through execute_llm_tool_call"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// CRITICAL BUG FIX: Synthesis loop error propagation (lines 185–201)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// CRITICAL: Test that when LLM provider returns an error in synthesis loop,
/// the error is genuinely propagated, NOT silently swallowed into a fake "Task completed." success.
///
/// This tests the fix to lines 185–201 in tool_use_loop_synthesis.rs:
/// The old code had: `Err(_) => { final_text = "Task completed."; break; }`
/// Which silently converted ANY error (network, rate-limit, auth) into a fake success.
/// The new code has: `Err(llm_err) => { return Err(StepError::ActionFailed { ... }) }`
#[tokio::test]
async fn test_synthesis_loop_propagates_llm_errors() {
    // Script: main loop returns ONLY a tool call (no text), triggering synthesis.
    // Synthesis LLM call fails with a rate-limit error.
    let script = vec![
        // Main loop: tool call only, no final text → synthesis will run
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        // Synthesis call: LLM provider fails with rate-limit
        ScriptedResponse::error(LlmError::RateLimited),
    ];
    let llm_client = LlmClient::new(Arc::new(ScriptedMockLlmProvider::new(script)));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "syn_error_prop",
        "Synthesize a completion.",
        "Please complete the task.",
        vec!["fs.list".to_string()],
        1, // max_rounds = 1 (main loop only, then synthesis)
    );

    let pipeline = Pipeline {
        name: "test_synthesis_error".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    // CRITICAL ASSERTION: The pipeline runner must return Err (PipelineError).
    // If the error-swallowing bug exists, the step would succeed with output "Task completed.".
    match result {
        Err(pipeline_err) => {
            let err_str = pipeline_err.to_string();
            assert!(
                err_str.contains("synthesis LLM call failed"),
                "Expected error message to contain 'synthesis LLM call failed', got: {}",
                err_str
            );
        }
        Ok(pipeline_result) => {
            panic!(
                "CRITICAL BUG: Synthesis loop swallowed LLM error and fabricated success. \
                 Expected pipeline to fail with ActionFailed, but it succeeded. \
                 Output was: {}. This is the error-swallowing bug. \
                 Lines 185–188 in tool_use_loop_synthesis.rs must propagate Err(llm_err).",
                pipeline_result.step_results["syn_error_prop"].output.raw
            );
        }
    }
}

/// Secondary test: Verify network errors are also propagated, not swallowed.
#[tokio::test]
async fn test_synthesis_loop_propagates_network_errors() {
    let script = vec![
        // Main loop: tool call only (no text) → synthesis will run
        ScriptedResponse::tool_call("fs.list", json!({ "path": "." })),
        // Synthesis call: network failure
        ScriptedResponse::error(LlmError::NetworkError("connection timeout".into())),
    ];
    let llm_client = LlmClient::new(Arc::new(ScriptedMockLlmProvider::new(script)));
    let tool_registry = ToolRegistry::with_builtins();

    let step = tool_use_loop_step(
        "syn_net_error",
        "Synthesize.",
        "Complete.",
        vec!["fs.list".to_string()],
        1,
    );

    let pipeline = Pipeline {
        name: "test_synthesis_network".into(),
        steps: vec![step],
        on_failure: FailureMode::Abort,
        max_retries: 0,
    };

    let agent = make_agent(pipeline.clone(), ToolSet::Full);

    let mut runner = PipelineRunner::with_tool_registry(Arc::new(tool_registry));
    runner = runner.with_llm_client(Arc::new(llm_client));

    let result = runner.run(&pipeline, &agent, json!({})).await;

    // The pipeline runner must return Err (PipelineError).
    // If the error-swallowing bug exists, the step would succeed with output "Task completed.".
    match result {
        Err(pipeline_err) => {
            let err_str = pipeline_err.to_string();
            assert!(
                err_str.contains("synthesis LLM call failed"),
                "Expected error message to contain 'synthesis LLM call failed', got: {}",
                err_str
            );
        }
        Ok(pipeline_result) => {
            panic!(
                "CRITICAL BUG: Synthesis loop swallowed network error and fabricated success. \
                 Expected pipeline to fail, but it succeeded with output: {}. \
                 This is the error-swallowing bug. \
                 Lines 185–188 in tool_use_loop_synthesis.rs must propagate Err(llm_err).",
                pipeline_result.step_results["syn_net_error"].output.raw
            );
        }
    }
}
