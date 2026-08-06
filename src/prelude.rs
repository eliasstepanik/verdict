//! Prelude: commonly used types and traits

// Phase 11: Streaming, conversation history, LlmJudge
pub use crate::llm::{
    ChatMessage, ChatRole, ConversationRegistry, LlmChunk, LlmClient, LlmError, LlmProvider,
    LlmRequest, LlmResponse, LlmUsage, MessageHistory, OpenAiCompatibleProvider, ProviderSpec,
    ToolCall, ToolSchema,
};

// Phase 9: Advanced Execution
pub use crate::action::RemoteAgentError;
pub use crate::agent::RemoteAgentClient;
pub use crate::audit::MonitoringServer;
pub use crate::pipeline::{HotReloadHandle, Plugin, PluginError, PluginRegistry};

// Phase 14: Cancellation and Interrupt
pub use crate::cancel::CancellationToken;

// Phase 8: Self-Improvement
pub use crate::eval::{
    EvalError, EvaluationCase, EvaluationExpected, EvaluationResult, EvaluationRunner,
    EvaluationSuite, EvaluationSuiteResult,
};

// Phase F: Evaluation Polish
pub use crate::eval::{
    AnswerRelevancyScorer, CustomScorer, EvaluationDataset, Experiment, ExperimentDiff,
    ExperimentRunner, RubricItem, Scorer, ScorerConfig, ScorerError, ScorerResult, ToxicityScorer,
};

pub use crate::self_update::{
    SelfUpdateConfig, SelfUpdateEngine, SelfUpdateError, SelfUpdateProposal, SelfUpdateResult,
};

// Phase 7: Safety and Production
pub use crate::action::StopCondition;
pub use crate::audit::{call_tree_from_audit_log, CallTreeNode, CallTreeStatus};
pub use crate::budget::{BudgetError, BudgetTracker, RateLimiter};
pub use crate::context::SerializableStepContext;
pub use crate::injection::{
    InjectionResult, InjectionScanner, RiskLevel, SecretMatch, SecretScanner, SecretScannerConfig,
};

// Phase A: Quick Wins
pub use crate::action::{
    DelegationContext, DelegationDecision, DelegationFeedback, DelegationResult, IterationContext,
    IterationDecision,
};

// Core types
pub use crate::action::{
    DelegationPolicy, IterationFailureMode, MemoryIsolation, SkillMode, StepAction, StepError,
    StepOutput,
};
pub use crate::agent::{
    Agent, AgentPolicy, AgentVersion, FilesystemPolicy, NetworkPolicy, WorkspaceIsolation,
};
pub use crate::audit::{AuditEntry, AuditEvent, AuditLog};
pub use crate::context::{
    BudgetState, ContextStore, ContextStoreError, PipelineTrace, RequestContext, StepContext,
    StepResult, TraceEntry,
};
pub use crate::guards::GuardPhase;
pub use crate::guards::{Guard, GuardEngine, GuardError, TestRunner};
pub use crate::mcp::{DiscoveredTool, McpClient, McpError, McpServerConfig, McpToolAdapter};
pub use crate::pipeline::{
    AgentStep, AgentStepBuilder, FailureMode, GuardProcessor, InjectionProtection, Pipeline,
    PipelineBuilder, ProcessorStrategy, ProcessorViolation,
};
pub use crate::registry::{AgentRegistry, SkillRegistry, ToolRegistry};
pub use crate::runner::{
    LogEntry, LogLevel, OutputEvent, OutputSink, PipelineError, PipelineResult, PipelineRunner,
    SuspendedState,
};
pub use crate::skills::builtin::{
    api_design, code_review, refactoring, rust_debugging, test_writing,
};
pub use crate::skills::{Skill, SkillEval, SkillExample, SkillSet};
pub use crate::tools::{DiagnosticEntry, DiagnosticSeverity, StructuredOutput};
pub use crate::tools::{
    FunctionTool, Tool, ToolChunk, ToolContext, ToolError, ToolOutput, ToolSource,
};
pub use crate::toolset::ToolSet;

pub use crate::verdict::{Verdict, VerdictEngine, VerdictError};

// Phase 16: Prompt Templates and Structured Output
pub use crate::prompt::{PromptError, PromptProvider, PromptSegment, PromptTemplate};

pub use crate::agents::{
    coder_agent, debugger_agent, orchestrator_agent, planner_agent, reflector_agent, reviewer_agent,
};

// Phase 13: Interactive Sessions
pub use crate::session::{
    Attachment, ConversationHistory, ConversationMessage, ConversationRole, Session, SessionError,
    SessionId, SessionMeta, SessionPolicy, SessionRunner, SessionStatus, TokenUsage, TurnContent,
    TurnResult, UserTurn,
};

// Phase 15: Server/Daemon Mode
pub use crate::server::{
    AgentServer, ClientRequest, ServerError, ServerEvent, ServerPolicy, ServerTransport,
    StdioTransport,
};

// Phase B: Developer Experience Improvements
pub use crate::config::{
    AgentConfig, ConfigError, DevConfig, ObservabilityConfig, ProjectConfig, VerdictConfig,
};

// Phase C1: Multi-Tier Memory System
pub use crate::memory::{MemoryError, MemoryMessage, MemoryRole, MemoryStore, SemanticResult};
