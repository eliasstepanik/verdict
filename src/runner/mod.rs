use std::sync::Arc;

mod types;
mod budget;
mod delegation;
mod execution;

pub use types::{PipelineError, PipelineResult, OutputEvent, OutputSink, LogEntry, LogLevel, SuspendedState};

// These are re-exported for use within impl blocks on PipelineRunner
#[allow(unused_imports)]
pub(crate) use budget::*;
#[allow(unused_imports)]
pub(crate) use delegation::*;
#[allow(unused_imports)]
pub(crate) use execution::*;

/// Executor for pipelines with guards, verdicts, and audit logging
#[derive(Clone)]
pub struct PipelineRunner {
    pub audit_log: crate::audit::AuditLog,
    pub tool_registry: Arc<crate::registry::ToolRegistry>,
    pub agent_registry: Arc<crate::registry::AgentRegistry>,
    pub skill_registry: Arc<crate::skills::registry::SkillRegistry>,
    pub llm_client: Option<Arc<crate::llm::LlmClient>>,
    pub output_sink: Option<Arc<dyn OutputSink>>,
    pub conversation_registry: Arc<std::sync::Mutex<crate::llm::ConversationRegistry>>,
    pub context_store: Option<std::sync::Arc<crate::context::ContextStore>>,
    pub plugin_registry: Arc<crate::pipeline::PluginRegistry>,
    /// LLM client for auto-generating conversation titles (Phase A8)
    pub auto_title_llm: Option<Arc<crate::llm::LlmClient>>,
    /// Multi-tier memory store (Phase C1)
    pub memory: Option<Arc<dyn crate::memory::MemoryStore>>,
}

impl PipelineRunner {
    pub fn new() -> Self {
        Self {
            audit_log: crate::audit::AuditLog::new(),
            tool_registry: Arc::new(crate::registry::ToolRegistry::with_builtins()),
            agent_registry: Arc::new(crate::registry::AgentRegistry::new()),
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            context_store: None,
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            plugin_registry: Arc::new(crate::pipeline::PluginRegistry::new()),
            auto_title_llm: None,
            memory: None,
        }
    }

    pub fn with_tool_registry(tool_registry: Arc<crate::registry::ToolRegistry>) -> Self {
        Self {
            audit_log: crate::audit::AuditLog::new(),
            tool_registry,
            agent_registry: Arc::new(crate::registry::AgentRegistry::new()),
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            plugin_registry: Arc::new(crate::pipeline::PluginRegistry::new()),
            context_store: None,
            auto_title_llm: None,
            memory: None,
        }
    }

    pub fn with_agent_registry(agent_registry: Arc<crate::registry::AgentRegistry>) -> Self {
        Self {
            audit_log: crate::audit::AuditLog::new(),
            tool_registry: Arc::new(crate::registry::ToolRegistry::with_builtins()),
            agent_registry,
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            plugin_registry: Arc::new(crate::pipeline::PluginRegistry::new()),
            context_store: None,
            auto_title_llm: None,
            memory: None,
        }
    }

    pub fn with_registries(
        tool_registry: Arc<crate::registry::ToolRegistry>,
        agent_registry: Arc<crate::registry::AgentRegistry>,
    ) -> Self {
        Self {
            audit_log: crate::audit::AuditLog::new(),
            tool_registry,
            agent_registry,
            skill_registry: Arc::new(crate::skills::registry::SkillRegistry::new()),
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            plugin_registry: Arc::new(crate::pipeline::PluginRegistry::new()),
            context_store: None,
            auto_title_llm: None,
            memory: None,
        }
    }

    pub fn with_skill_registry(
        skill_registry: Arc<crate::skills::registry::SkillRegistry>,
    ) -> Self {
        Self {
            audit_log: crate::audit::AuditLog::new(),
            tool_registry: Arc::new(crate::registry::ToolRegistry::with_builtins()),
            agent_registry: Arc::new(crate::registry::AgentRegistry::new()),
            skill_registry,
            llm_client: None,
            output_sink: None,
            conversation_registry: Arc::new(std::sync::Mutex::new(
                crate::llm::ConversationRegistry::new(),
            )),
            plugin_registry: Arc::new(crate::pipeline::PluginRegistry::new()),
            context_store: None,
            auto_title_llm: None,
            memory: None,
        }
    }

    pub fn with_llm_client(mut self, client: Arc<crate::llm::LlmClient>) -> Self {
        self.llm_client = Some(client);
        self
    }

    pub fn with_output_sink(mut self, sink: Arc<dyn OutputSink>) -> Self {
        self.output_sink = Some(sink);
        self
    }

    pub fn with_context_store(mut self, dir: std::path::PathBuf) -> Self {
        self.context_store = Some(std::sync::Arc::new(crate::context::ContextStore::new(dir)));
        self
    }

    pub fn with_plugin_registry(mut self, registry: Arc<crate::pipeline::PluginRegistry>) -> Self {
        self.plugin_registry = registry;
        self
    }

    /// Set an LLM client for auto-generating conversation titles (Phase A8).
    /// After the first LLM call in a new conversation, a title will be generated
    /// asynchronously and stored in the ConversationRegistry.
    pub fn with_auto_title_model(mut self, client: Arc<crate::llm::LlmClient>) -> Self {
        self.auto_title_llm = Some(client);
        self
    }

    /// Set a multi-tier memory store for the runner (Phase C1).
    pub fn with_memory(mut self, store: Arc<dyn crate::memory::MemoryStore>) -> Self {
        self.memory = Some(store);
        self
    }
}

impl Default for PipelineRunner {
    fn default() -> Self {
        Self::new()
    }
}
