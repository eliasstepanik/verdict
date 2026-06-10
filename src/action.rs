use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

use crate::context::StepContext;
use crate::guard::GuardError;
use crate::verdict::VerdictError;
use crate::pipeline::Pipeline;
use crate::guard::Guard;

/// Specification for an LLM provider and model
#[derive(Debug, Clone)]
pub struct ProviderSpec {
    pub model: String,
    pub provider: String,
}

/// Policy controlling agent delegation
#[derive(Debug, Clone)]
pub struct DelegationPolicy {
    pub max_depth: u32,
    pub allowed_agents: Vec<String>,
    pub require_output_schema: bool,
    pub inherit_tool_scope: bool,
    pub inherit_budget: bool,
    pub require_user_approval: bool,
}

/// How to handle iteration failure in LoopUntil
#[derive(Debug, Clone)]
pub enum IterationFailureMode {
    /// Retry the iteration body immediately
    Retry,
    /// Skip this iteration and move to the next
    Skip,
    /// Abort the entire loop and fail
    Abort,
}

/// Error from remote agent execution
#[derive(Error, Debug)]
pub enum RemoteAgentError {
    #[error("request failed: {0}")]
    RequestFailed(String),

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("network error: {0}")]
    NetworkError(String),

    #[error("timeout")]
    Timeout,
}

/// Skill execution mode
#[derive(Debug, Clone)]
pub enum SkillMode {
    /// Inject skill instructions into the current step's LLM prompt
    PromptOnly,
    /// Run the skill's pipeline as a sub-pipeline
    Pipeline,
    /// Let the runtime choose between prompt-only and pipeline
    Auto,
}

/// Output from a step action
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StepOutput {
    /// Raw output string
    pub raw: String,
    /// Parsed structured output (if applicable)
    pub parsed: Option<Value>,
}

impl StepOutput {
    pub fn new(raw: String) -> Self {
        Self {
            raw,
            parsed: None,
        }
    }

    pub fn with_parsed(raw: String, parsed: Value) -> Self {
        Self {
            raw,
            parsed: Some(parsed),
        }
    }
}

/// A step action to be executed
#[derive(Clone)]
pub enum StepAction {
    /// Call an LLM with a prompt
    LlmCall {
        system: String,
        user: String,
        model: Option<ProviderSpec>,
        /// Optional conversation ID for multi-turn interactions
        conversation_id: Option<String>,
        /// Whether to append the user message and assistant response to conversation history
        append_to_history: bool,
    },

    /// Run a tool directly
    ToolCall {
        tool: String,
        args: Value,
    },

    /// Delegate to a named agent
    DelegateAgent {
        agent: String,
        input: Value,
        expected_output_schema: Option<Value>,
        delegation_policy: DelegationPolicy,
    },

    /// Execute a sub-pipeline
    SubPipeline(Box<Pipeline>),

    /// Loop/iterate until a condition is met
    LoopUntil {
        body: Box<StepAction>,
        condition: Guard,
        max_iterations: u32,
        on_iteration_failure: IterationFailureMode,
    },

    /// Execute arbitrary Rust code
    Custom(Arc<dyn Fn(&StepContext) -> Result<StepOutput, StepError> + Send + Sync>),

    /// Ask the user for input
    UserInput {
        prompt: String,
        schema: Option<Value>,
    },

    /// Use a registered skill
    UseSkill {
        skill: String,
        input: Value,
        mode: SkillMode,
    },

    /// Conditional branching: evaluate condition against previous output
    Branch {
        condition: String,
        if_true: Box<StepAction>,
        if_false: Option<Box<StepAction>>,
    },

    /// Execute a step on a remote agent endpoint
    RemoteAgent {
        endpoint: String,
        agent_name: String,
        payload: Value,
    },

    /// Call an LLM and stream the response via the runner's OutputSink.
    /// Guards and verdicts still run against the fully assembled output after streaming completes.
    LlmCallStreaming {
        system: String,
        user: String,
        model: Option<ProviderSpec>,
    },

    /// ReAct tool-use loop: iterate until stop condition met
    ToolUseLoop {
        system: String,
        user: String,
        model: ProviderSpec,
        tools: Vec<String>,
        max_rounds: usize,
        stop_condition: StopCondition,
    },
}

/// Stop condition for ToolUseLoop
#[derive(Debug, Clone)]
pub enum StopCondition {
    /// Stop when LLM returns no tool calls (text-only response)
    TextOnly,
    /// Stop when output matches a regex pattern
    Pattern(String),
    /// Always run to max_rounds
    MaxRounds,
}

impl std::fmt::Debug for StepAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepAction::LlmCall {
                system,
                user,
                model,
                conversation_id,
                append_to_history,
            } => f
                .debug_struct("LlmCall")
                .field("system", system)
                .field("user", user)
                .field("model", model)
                .field("conversation_id", conversation_id)
                .field("append_to_history", append_to_history)
                .finish(),
            StepAction::ToolCall { tool, args } => f
                .debug_struct("ToolCall")
                .field("tool", tool)
                .field("args", args)
                .finish(),
            StepAction::DelegateAgent {
                agent,
                input,
                expected_output_schema,
                delegation_policy,
            } => f
                .debug_struct("DelegateAgent")
                .field("agent", agent)
                .field("input", input)
                .field("expected_output_schema", expected_output_schema)
                .field("delegation_policy", delegation_policy)
                .finish(),
            StepAction::SubPipeline(pipeline) => f
                .debug_tuple("SubPipeline")
                .field(pipeline)
                .finish(),
            StepAction::LoopUntil {
                body: _,
                condition,
                max_iterations,
                on_iteration_failure,
            } => f
                .debug_struct("LoopUntil")
                .field("body", &"<action>")
                .field("condition", condition)
                .field("max_iterations", max_iterations)
                .field("on_iteration_failure", on_iteration_failure)
                .finish(),
            StepAction::Custom(_) => f.debug_tuple("Custom").field(&"<fn>").finish(),
            StepAction::UserInput { prompt, schema } => f
                .debug_struct("UserInput")
                .field("prompt", prompt)
                .field("schema", schema)
                .finish(),
            StepAction::UseSkill { skill, input, mode } => f
                .debug_struct("UseSkill")
                .field("skill", skill)
                .field("input", input)
                .field("mode", mode)
                .finish(),
            StepAction::Branch {
                condition,
                if_true: _,
                if_false: _,
            } => f
                .debug_struct("Branch")
                .field("condition", condition)
                .field("if_true", &"<action>")
                .field("if_false", &"<action?>")
                .finish(),
            StepAction::RemoteAgent {
                endpoint,
                agent_name,
                payload,
            } => f
                .debug_struct("RemoteAgent")
                .field("endpoint", endpoint)
                .field("agent_name", agent_name)
                .field("payload", payload)
                .finish(),
            StepAction::LlmCallStreaming { system, user, model } => f
                .debug_struct("LlmCallStreaming")
                .field("system", system)
                .field("user", user)
                .field("model", model)
                .finish(),
            StepAction::ToolUseLoop {
                system,
                user,
                model,
                tools,
                max_rounds,
                stop_condition,
            } => f
                .debug_struct("ToolUseLoop")
                .field("system", system)
                .field("user", user)
                .field("model", model)
                .field("tools", tools)
                .field("max_rounds", max_rounds)
                .field("stop_condition", stop_condition)
                .finish(),
        }
    }
}

/// Error from executing a step action
#[derive(Error, Debug)]
pub enum StepError {
    #[error("action failed: {reason}")]
    ActionFailed { reason: String },

    #[error("guard failed: {0}")]
    GuardFailed(#[from] GuardError),

    #[error("verdict failed: {0}")]
    VerdictFailed(#[from] VerdictError),

    #[error("awaiting user approval: {prompt}")]
    AwaitingApproval { prompt: &'static str },

    #[error("not implemented: {0}")]
    NotImplemented(String),
}
