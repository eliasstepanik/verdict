use serde_json::Value;
use std::sync::Arc;
use thiserror::Error;

use crate::context::StepContext;
use crate::guards::Guard;
use crate::guards::GuardError;
use crate::pipeline::Pipeline;
use crate::verdict::VerdictError;

/// Specification for an LLM provider and model
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderSpec {
    pub model: String,
    pub provider: String,
}

/// Memory isolation strategy for delegated agents — Phase D
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub enum MemoryIsolation {
    /// Fresh conversation_id per delegation (default)
    Isolated,
    /// Share parent's conversation_id
    Shared,
    /// "{parent_conversation_id}/{depth}/{agent_name}/{step_name}"
    NamespacedByAgent,
}

impl Default for MemoryIsolation {
    fn default() -> Self {
        Self::Isolated
    }
}

/// Policy controlling agent delegation
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct DelegationPolicy {
    pub max_depth: u32,
    pub allowed_agents: Vec<String>,
    pub require_output_schema: bool,
    pub inherit_tool_scope: bool,
    pub inherit_budget: bool,
    pub require_user_approval: bool,
    /// Memory isolation strategy for delegated agents — Phase D1
    pub memory_isolation: MemoryIsolation,

    /// Hook called before delegation starts; can modify input or reject delegation
    #[serde(skip)]
    pub on_delegation_start:
        Option<Arc<dyn Fn(&DelegationContext) -> DelegationDecision + Send + Sync>>,

    /// Hook called after delegation completes; can inject feedback or bail out
    #[serde(skip)]
    pub on_delegation_complete:
        Option<Arc<dyn Fn(&DelegationResult) -> DelegationFeedback + Send + Sync>>,

    /// Hook called after each LoopUntil iteration completes
    #[serde(skip)]
    pub on_iteration_complete:
        Option<Arc<dyn Fn(&IterationContext) -> IterationDecision + Send + Sync>>,

    /// Optional message filter to transform conversation history before passing to child agent
    #[serde(skip)]
    pub message_filter: Option<
        Arc<dyn Fn(&crate::llm::MessageHistory) -> crate::llm::MessageHistory + Send + Sync>,
    >,
}

impl Default for DelegationPolicy {
    fn default() -> Self {
        Self {
            max_depth: 3,
            allowed_agents: Vec::new(),
            require_output_schema: false,
            inherit_tool_scope: true,
            inherit_budget: true,
            require_user_approval: false,
            memory_isolation: MemoryIsolation::Isolated,
            on_delegation_start: None,
            on_delegation_complete: None,
            on_iteration_complete: None,
            message_filter: None,
        }
    }
}

impl std::fmt::Debug for DelegationPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegationPolicy")
            .field("max_depth", &self.max_depth)
            .field("allowed_agents", &self.allowed_agents)
            .field("require_output_schema", &self.require_output_schema)
            .field("inherit_tool_scope", &self.inherit_tool_scope)
            .field("inherit_budget", &self.inherit_budget)
            .field("require_user_approval", &self.require_user_approval)
            .field("memory_isolation", &self.memory_isolation)
            .field(
                "on_delegation_start",
                &self.on_delegation_start.as_ref().map(|_| "<function>"),
            )
            .field(
                "on_delegation_complete",
                &self.on_delegation_complete.as_ref().map(|_| "<function>"),
            )
            .field(
                "on_iteration_complete",
                &self.on_iteration_complete.as_ref().map(|_| "<function>"),
            )
            .field(
                "message_filter",
                &self.message_filter.as_ref().map(|_| "<function>"),
            )
            .finish()
    }
}

/// Context passed to delegation hooks (A2)
#[derive(Debug, Clone)]
pub struct DelegationContext {
    pub agent: String,
    pub input: Value,
    pub depth: u32,
}

/// Decision from delegation start hook
#[derive(Debug)]
pub enum DelegationDecision {
    Proceed,
    Reject { reason: String },
    ModifyInput(Value),
}

/// Result from delegation completion
#[derive(Debug, Clone)]
pub struct DelegationResult {
    pub agent: String,
    pub output: StepOutput,
    pub success: bool,
}

/// Feedback from delegation complete hook
#[derive(Debug)]
pub enum DelegationFeedback {
    Continue,
    Bail { reason: String },
    InjectFeedback(String),
}

/// Context for loop iteration hook
#[derive(Debug, Clone)]
pub struct IterationContext {
    pub iteration: u32,
    pub agent: String,
    pub output: StepOutput,
}

/// Decision for loop iteration
#[derive(Debug)]
pub enum IterationDecision {
    Continue,
    Stop,
}

/// How to handle iteration failure in LoopUntil
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]

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
    /// Skill evaluation result (if applicable)
    pub eval_result: Option<String>,
}

impl StepOutput {
    pub fn new(raw: String) -> Self {
        Self {
            raw,
            parsed: None,
            eval_result: None,
        }
    }

    pub fn with_parsed(raw: String, parsed: Value) -> Self {
        Self {
            raw,
            parsed: Some(parsed),
            eval_result: None,
        }
    }

    pub fn set_eval_result(&mut self, eval_result: String) {
        self.eval_result = Some(eval_result);
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
    ToolCall { tool: String, args: Value },

    /// Delegate to a named agent
    DelegateAgent {
        agent: String,
        input: Value,
        expected_output_schema: Option<Value>,
        delegation_policy: DelegationPolicy,
        /// If true, spawn child agent as fire-and-forget task (Phase D3)
        detached: bool,
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
        /// Optional conversation ID for multi-turn interactions
        conversation_id: Option<String>,
        /// Whether to append the user message and assistant response to conversation history
        append_to_history: bool,
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

    /// Sleep for a duration (A3)
    Sleep { duration_ms: u64 },

    /// Sleep until a specific timestamp (A3)
    SleepUntil {
        timestamp: chrono::DateTime<chrono::Utc>,
    },

    /// ForEach: iterate over array items with optional concurrency (A4)
    ForEach {
        input_array_key: String, // key into step_results for the array
        body: Box<StepAction>,   // executed for each item
        concurrency: usize,      // 1 = sequential, >1 = concurrent with semaphore
        collect_results: bool,   // if true, output is JSON array of all results
    },

    /// Suspend pipeline execution and save state for later resume (Phase D4)
    Suspend {
        reason: String,
        resume_schema: Option<Value>, // JSON schema the resume_data must match
        timeout_seconds: Option<u64>,
    },

    /// Self-correction loop: execute body, judge output against rubric, iterate (Phase F)
    RubricLoop {
        body: Box<StepAction>,
        rubric: Vec<crate::eval::RubricItem>,
        max_iterations: u32,
        judge_model: Option<ProviderSpec>, // None = use runner's default
    },
}

/// Stop condition for ToolUseLoop
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StopCondition {
    /// Stop when LLM returns no tool calls (text-only response)
    TextOnly,
    /// Stop when output contains a substring pattern (case-sensitive; not a regex)
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
            } => {
                let sys_preview = if system.len() > 50 {
                    format!("{}...", &system[..50])
                } else {
                    system.clone()
                };
                let usr_preview = if user.len() > 50 {
                    format!("{}...", &user[..50])
                } else {
                    user.clone()
                };
                f.debug_struct("LlmCall")
                    .field("system", &sys_preview)
                    .field("user", &usr_preview)
                    .field("model", model)
                    .field("conversation_id", conversation_id)
                    .field("append_to_history", append_to_history)
                    .finish()
            }

            StepAction::DelegateAgent {
                agent,
                input,
                expected_output_schema,
                delegation_policy,
                detached,
            } => f
                .debug_struct("DelegateAgent")
                .field("agent", agent)
                .field("input", input)
                .field("expected_output_schema", expected_output_schema)
                .field("delegation_policy", delegation_policy)
                .field("detached", detached)
                .finish(),
            StepAction::SubPipeline(pipeline) => {
                f.debug_tuple("SubPipeline").field(pipeline).finish()
            }
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
            StepAction::LlmCallStreaming {
                system,
                user,
                model,
                conversation_id,
                append_to_history,
            } => f
                .debug_struct("LlmCallStreaming")
                .field("system", system)
                .field("user", user)
                .field("model", model)
                .field("conversation_id", conversation_id)
                .field("append_to_history", append_to_history)
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
            StepAction::ToolCall { tool, args } => f
                .debug_struct("ToolCall")
                .field("tool", tool)
                .field("args", args)
                .finish(),

            StepAction::Sleep { duration_ms } => f
                .debug_struct("Sleep")
                .field("duration_ms", duration_ms)
                .finish(),

            StepAction::SleepUntil { timestamp } => f
                .debug_struct("SleepUntil")
                .field("timestamp", timestamp)
                .finish(),

            StepAction::ForEach {
                input_array_key,
                body: _,
                concurrency,
                collect_results,
            } => f
                .debug_struct("ForEach")
                .field("input_array_key", input_array_key)
                .field("body", &"<action>")
                .field("concurrency", concurrency)
                .field("collect_results", collect_results)
                .finish(),

            StepAction::Suspend {
                reason,
                resume_schema,
                timeout_seconds,
            } => f
                .debug_struct("Suspend")
                .field("reason", reason)
                .field("resume_schema", resume_schema)
                .field("timeout_seconds", timeout_seconds)
                .finish(),

            StepAction::RubricLoop {
                body: _,
                rubric,
                max_iterations,
                judge_model,
            } => f
                .debug_struct("RubricLoop")
                .field("body", &"<action>")
                .field("rubric", rubric)
                .field("max_iterations", max_iterations)
                .field("judge_model", judge_model)
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
    #[error("remote agent failed: {0}")]
    RemoteAgentFailed(#[from] RemoteAgentError),
}
