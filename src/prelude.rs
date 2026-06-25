//! Prelude: commonly used types and traits

// Phase 11: Streaming, conversation history, LlmJudge
pub use crate::llm::{
    LlmProvider, LlmClient, LlmRequest, LlmResponse, LlmError, LlmUsage, ProviderSpec,
    LlmChunk, ChatRole, ChatMessage, MessageHistory, ConversationRegistry, OpenAiCompatibleProvider,
    ToolCall,

    ToolSchema,
};

// Phase 9: Advanced Execution
pub use crate::action::RemoteAgentError;
pub use crate::agent::RemoteAgentClient;
pub use crate::pipeline::{HotReloadHandle, Plugin, PluginError, PluginRegistry};
pub use crate::audit::MonitoringServer;

// Phase 14: Cancellation and Interrupt
pub use crate::cancel::CancellationToken;

// Phase 8: Self-Improvement
pub use crate::eval::{
    EvaluationSuite, EvaluationCase, EvaluationExpected, EvaluationResult, EvaluationSuiteResult,
    EvaluationRunner, EvalError,
};

// Phase F: Evaluation Polish
pub use crate::eval::{
    Scorer, ScorerResult, ScorerError, ScorerConfig,
    AnswerRelevancyScorer, ToxicityScorer, CustomScorer,
    RubricItem,
    EvaluationDataset, Experiment, ExperimentDiff, ExperimentRunner,
};

pub use crate::self_update::{
    SelfUpdateConfig, SelfUpdateProposal, SelfUpdateResult, SelfUpdateEngine, SelfUpdateError,
};

// Phase 7: Safety and Production
pub use crate::injection::{InjectionScanner, InjectionResult, SecretScanner, SecretMatch, RiskLevel, SecretScannerConfig};
pub use crate::budget::{BudgetTracker, RateLimiter, BudgetError};
pub use crate::context::SerializableStepContext;
pub use crate::audit::{CallTreeNode, CallTreeStatus, call_tree_from_audit_log};
pub use crate::action::StopCondition;

// Phase A: Quick Wins
pub use crate::action::{
    DelegationContext, DelegationDecision, DelegationResult, DelegationFeedback,
    IterationContext, IterationDecision,
};

// Core types
pub use crate::action::{
    DelegationPolicy, MemoryIsolation, IterationFailureMode, SkillMode, StepAction, StepError,
    StepOutput,
};
pub use crate::agent::{
    Agent, AgentPolicy, AgentVersion, FilesystemPolicy, NetworkPolicy,
    WorkspaceIsolation,
};
pub use crate::skills::{Skill, SkillExample, SkillEval, SkillSet};
pub use crate::audit::{AuditEntry, AuditEvent, AuditLog};
pub use crate::context::{BudgetState, PipelineTrace, StepContext, StepResult, TraceEntry, ContextStore, ContextStoreError, RequestContext};
pub use crate::guards::{Guard, GuardEngine, GuardError, TestRunner};
pub use crate::mcp::{McpClient, McpError, McpServerConfig, McpToolAdapter, DiscoveredTool};
pub use crate::pipeline::{AgentStep, FailureMode, InjectionProtection, Pipeline, PipelineBuilder, AgentStepBuilder, GuardProcessor, ProcessorViolation, ProcessorStrategy};
pub use crate::registry::{AgentRegistry, SkillRegistry, ToolRegistry};
pub use crate::skills::builtin::{api_design, code_review, refactoring, rust_debugging, test_writing};
pub use crate::runner::{PipelineError, PipelineResult, PipelineRunner, OutputSink, OutputEvent, LogEntry, LogLevel, SuspendedState};
pub use crate::guards::GuardPhase;
pub use crate::toolset::ToolSet;
pub use crate::tools::{Tool, ToolContext, ToolError, ToolOutput, ToolSource, FunctionTool, ToolChunk};
pub use crate::tools::{DiagnosticSeverity, DiagnosticEntry, StructuredOutput};

pub use crate::verdict::{Verdict, VerdictEngine, VerdictError};

// Phase 16: Prompt Templates and Structured Output
pub use crate::prompt::{PromptTemplate, PromptSegment, PromptProvider, PromptError};

pub use crate::agents::{
    planner_agent, coder_agent, reviewer_agent, debugger_agent, reflector_agent, orchestrator_agent,
};

// Phase 13: Interactive Sessions
pub use crate::session::{
    Session, SessionId, SessionRunner, SessionPolicy, SessionMeta, SessionStatus,
    SessionError, UserTurn, TurnContent, TurnResult, TokenUsage,
    ConversationHistory, ConversationMessage, ConversationRole, Attachment,
};

// Phase 15: Server/Daemon Mode
pub use crate::server::{
    AgentServer, ServerPolicy, ServerTransport, ServerEvent, ClientRequest, ServerError, StdioTransport,
};


// Phase B: Developer Experience Improvements
pub use crate::config::{VerdictConfig, ProjectConfig, DevConfig, AgentConfig, ObservabilityConfig, ConfigError};

// Phase C1: Multi-Tier Memory System
pub use crate::memory::{MemoryStore, MemoryMessage, MemoryRole, SemanticResult, MemoryError};

