use crate::config::AppConfig;
use serde_json::json;
use std::sync::Arc;
use verdict::eval::{RubricItem, ScorerConfig, ToxicityScorer};
use verdict::pipeline::{GuardProcessor, ProcessorStrategy};
use verdict::prelude::*;

/// Build the main interactive assistant agent.
/// Single ToolUseLoop step — Claude decides whether to use tools or reply directly.
pub fn build_assistant_agent(config: &AppConfig, agent_name: &str) -> Agent {
    let system = config.effective_system_prompt();
    let tools = vec![
        "fs.read".to_string(),
        "fs.list".to_string(),
        "fs.write".to_string(),
        "search.files".to_string(),
        "search.grep".to_string(),
        "shell.run".to_string(),
        "shell.cargo_check".to_string(),
        "shell.cargo_test".to_string(),
    ];
    Agent {
        name: agent_name.to_string(),
        description: "Interactive assistant with filesystem and shell tools".to_string(),
        pipeline: Pipeline {
            name: format!("{}-pipeline", agent_name),
            steps: vec![AgentStep {
                name: "act".to_string(),
                guard_in: Guard::None,
                action: StepAction::ToolUseLoop {
                    system,
                    user: "{input}".to_string(),
                    model: ProviderSpec {
                        model: String::new(),
                        provider: String::new(),
                    },
                    tools: tools.clone(),
                    max_rounds: 10,
                    stop_condition: StopCondition::TextOnly,
                },
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),

                tools: ToolSet::Allow(tools.clone()),
                injection_protection: InjectionProtection::Strict,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            }],
            on_failure: FailureMode::Abort,
            max_retries: 1,
        },
        tools: ToolSet::Allow(tools),
        skills: SkillSet {
            skills: vec!["rust_debugging".to_string(), "code_review".to_string()],
        },
        policy: AgentPolicy {
            allow_self_update: false,
            ..AgentPolicy::default()
        },
        scorers: Vec::new(),
    }
}

/// Build the self-improvement pipeline (reflect + propose).

/// Build the self-improvement pipeline.
/// Single LlmCall step — reflects on the session and proposes one concrete improvement.
/// Returns JSON: {"finding": "...", "proposal": "...", "risk_level": "low|medium|high"}
pub fn build_improve_pipeline() -> Pipeline {
    Pipeline {
        name: "improve_pipeline".to_string(),
        steps: vec![
            AgentStep {
                name: "reflect_and_propose".to_string(),
                guard_in: Guard::None,
                action: StepAction::LlmCall {
                    system: "You are an AI assistant analyst. Your job is to reflect on an AI coding assistant's recent session and propose one concrete improvement.\n\nYou MUST respond with ONLY valid JSON — no markdown, no explanation, no code fences. Just raw JSON:\n{\"finding\": \"<what pattern or weakness you observed>\", \"proposal\": \"<one specific actionable improvement>\", \"risk_level\": \"low\"}".to_string(),
                    user: "The assistant just completed {input} turns helping a user with coding tasks. Based on typical patterns in AI coding assistants (tool use reliability, response quality, context handling), propose one improvement.\n\nRespond with ONLY this JSON (no other text):\n{\"finding\": \"...\", \"proposal\": \"...\", \"risk_level\": \"low\"}".to_string(),
                    model: None,
                    conversation_id: None,
                    append_to_history: false,
                },
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            },
        ],
        on_failure: FailureMode::Abort,
        max_retries: 1,
    }
}

/// Fallback echo agent when no LLM is configured.
pub fn build_echo_agent(agent_name: &str) -> Agent {
    Agent {
        name: agent_name.to_string(),
        description: "Echo agent (no LLM)".to_string(),
        pipeline: Pipeline {
            name: format!("{}-pipeline", agent_name),
            steps: vec![AgentStep {
                name: "respond".to_string(),
                guard_in: Guard::None,
                action: StepAction::Custom(Arc::new(|ctx| {
                    let input = ctx.input.as_str().unwrap_or("(no input)");
                    Ok(StepOutput::new(format!("Echo: {}", input)))
                })),
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            }],
            on_failure: FailureMode::Abort,
            max_retries: 0,
        },
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: Vec::new(),
    }
}

/// Memory-enhanced agent using PipelineBuilder DSL with guard processors and scorer.
#[allow(dead_code)]

pub fn build_memory_agent(config: &AppConfig, agent_name: &str) -> Agent {
    let system = config.effective_system_prompt();
    Agent {
        name: agent_name.to_string(),
        description: "Memory-enhanced agent with pipeline builder DSL".to_string(),
        pipeline: PipelineBuilder::new("memory-pipeline")
            .then(AgentStep {
                name: "understand".to_string(),
                guard_in: Guard::None,
                action: StepAction::LlmCall {
                    system: "You are an intent parser. Extract the user's core request."
                        .to_string(),
                    user: "Task: {input}".to_string(),
                    model: None,
                    conversation_id: None,
                    append_to_history: false,
                },
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::None,
                injection_protection: InjectionProtection::Strict,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            })
            .then(AgentStep {
                name: "act".to_string(),
                guard_in: Guard::StepPassed("understand".to_string()),
                action: StepAction::ToolUseLoop {
                    system,
                    user: "Task: {understand}\n\nOriginal request: {input}".to_string(),
                    model: ProviderSpec {
                        model: String::new(),
                        provider: String::new(),
                    },
                    tools: vec![
                        "fs.read".to_string(),
                        "fs.list".to_string(),
                        "fs.write".to_string(),
                    ],
                    max_rounds: 8,
                    stop_condition: StopCondition::TextOnly,
                },
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::Automated(Guard::NonEmptyOutput),
                tools: ToolSet::Allow(vec![
                    "fs.read".to_string(),
                    "fs.list".to_string(),
                    "fs.write".to_string(),
                ]),
                injection_protection: InjectionProtection::Strict,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![GuardProcessor::new("empty_check", Guard::NonEmptyOutput)
                    .with_strategy(ProcessorStrategy::Warn)],
            })
            .build(),
        tools: ToolSet::Allow(vec![
            "fs.read".to_string(),
            "fs.list".to_string(),
            "fs.write".to_string(),
        ]),
        skills: SkillSet {
            skills: vec!["rust_debugging".to_string()],
        },
        policy: AgentPolicy::default(),
        scorers: vec![ScorerConfig {
            scorer: Arc::new(ToxicityScorer::new()),
            sampling_rate: 1.0,
        }],
    }
}

/// Multi-agent delegation pipeline with shared memory.
#[allow(dead_code)]

pub fn build_multi_agent_pipeline(_primary_name: &str, helper_name: &str) -> Pipeline {
    PipelineBuilder::new("multi-agent-pipeline")
        .then(AgentStep {
            name: "delegate_to_helper".to_string(),
            guard_in: Guard::None,
            action: StepAction::DelegateAgent {
                agent: helper_name.to_string(),
                input: json!({ "task": "{input}" }),
                expected_output_schema: None,
                delegation_policy: DelegationPolicy {
                    max_depth: 2,
                    allowed_agents: vec![helper_name.to_string()],
                    require_output_schema: false,
                    inherit_tool_scope: true,
                    inherit_budget: true,
                    require_user_approval: false,
                    on_delegation_start: None,
                    on_delegation_complete: None,
                    on_iteration_complete: None,
                    message_filter: None,
                    memory_isolation: MemoryIsolation::Shared,
                },
                detached: false,
            },
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::Strict,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        })
        .then(AgentStep {
            name: "summarize_result".to_string(),
            guard_in: Guard::StepPassed("delegate_to_helper".to_string()),
            action: StepAction::Custom(Arc::new(|ctx| {
                let out = ctx
                    .step_results
                    .get("delegate_to_helper")
                    .map(|r| r.output.raw.clone())
                    .unwrap_or_default();
                Ok(StepOutput::new(format!("Summary:\n{}", out)))
            })),
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::Strict,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        })
        .build()
}

/// Evaluation pipeline with rubric-based self-correction loop.
#[allow(dead_code)]

pub fn build_eval_pipeline() -> Pipeline {
    PipelineBuilder::new("eval-pipeline")
        .then(AgentStep {
            name: "evaluate_with_rubric".to_string(),
            guard_in: Guard::None,
            action: StepAction::RubricLoop {
                body: Box::new(StepAction::Custom(Arc::new(|_ctx| {
                    Ok(StepOutput::new(
                        "This is a well-structured response.".to_string(),
                    ))
                }))),
                rubric: vec![
                    RubricItem {
                        criterion: "Response is complete".to_string(),
                        required: true,
                    },
                    RubricItem {
                        criterion: "Response is accurate".to_string(),
                        required: true,
                    },
                ],
                max_iterations: 3,
                judge_model: None,
            },
            guard_out: Guard::NonEmptyOutput,
            verdict: Verdict::Automated(Guard::NonEmptyOutput),
            tools: ToolSet::None,
            injection_protection: InjectionProtection::Strict,
            output_schema: None,
            dependencies: vec![],
            parallel: false,
            input_processors: vec![],
            output_processors: vec![],
        })
        .build()
}
