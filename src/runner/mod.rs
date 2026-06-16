use std::sync::Arc;

mod types;
mod context_helpers;
mod budget;
mod delegation;
mod execution;

pub use types::{PipelineError, PipelineResult, OutputEvent, OutputSink};

// These are re-exported for use within impl blocks on PipelineRunner
#[allow(unused_imports)]
pub(crate) use context_helpers::resolve_template;
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
}

impl Default for PipelineRunner {
    fn default() -> Self {
        Self::new()
    }
}
