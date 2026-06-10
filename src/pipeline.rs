use serde_json::Value;

use crate::action::StepAction;
use crate::guard::Guard;
use crate::verdict::Verdict;
use crate::toolset::ToolSet;

/// Injection protection strategy for a step
#[derive(Debug, Clone)]
pub enum InjectionProtection {
    /// No protection (Phase 2+: would scan for injection patterns)
    None,

    /// Strict protection (Phase 2+: would apply aggressive filtering)
    Strict,
}

/// How to handle step failure
#[derive(Debug, Clone)]
pub enum FailureMode {
    /// Stop the pipeline immediately
    Abort,

    /// Retry the step up to max_retries times
    Retry,

    /// Skip this step and continue to the next
    Skip,

    /// Execute a fallback pipeline
    Fallback(Box<Pipeline>),
}

/// A single step in a pipeline
#[derive(Debug, Clone)]
pub struct AgentStep {
    /// Step name
    pub name: String,

    /// Guard that must pass before executing this step
    pub guard_in: Guard,

    /// The action to execute
    pub action: StepAction,

    /// Guard that must pass after execution (before verdict)
    pub guard_out: Guard,

    /// Final verdict decision
    pub verdict: Verdict,

    /// Allowed tools for this step
    pub tools: ToolSet,

    /// Injection protection strategy
    pub injection_protection: InjectionProtection,

    /// Expected output schema (for validation and handoff)
    pub output_schema: Option<Value>,

    /// DAG dependencies: list of step names that must complete before this step
    pub dependencies: Vec<String>,

    /// Whether this step can be executed in parallel with other steps
    pub parallel: bool,
}

/// A composable pipeline of steps
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Pipeline name
    pub name: String,

    /// Ordered sequence of steps
    pub steps: Vec<AgentStep>,

    /// How to handle step failures
    pub on_failure: FailureMode,

    /// Maximum retries per step
    pub max_retries: u32,
}

/// Error from a plugin hook
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("plugin hook failed: {0}")]
    HookFailed(String),

    #[error("plugin execution error: {0}")]
    ExecutionError(String),
}

/// Context passed to plugin hooks
#[derive(Debug, Clone)]
pub struct StepContext;

/// A plugin that can hook into the pipeline execution lifecycle
#[async_trait::async_trait]
pub trait Plugin: Send + Sync {
    /// Plugin name
    fn name(&self) -> &str;

    /// Called when a step starts
    async fn on_step_start(&self, _ctx: &crate::context::StepContext) -> Result<(), PluginError> {
        Ok(())
    }

    /// Called when a step ends
    async fn on_step_end(
        &self,
        _ctx: &crate::context::StepContext,
        _result: &crate::action::StepOutput,
    ) -> Result<(), PluginError> {
        Ok(())
    }
}

/// Registry of plugins
pub struct PluginRegistry {
    plugins: Vec<std::sync::Arc<dyn Plugin>>,
}

impl PluginRegistry {
    /// Create a new plugin registry
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
        }
    }

    /// Register a plugin
    pub fn register(&mut self, plugin: std::sync::Arc<dyn Plugin>) {
        self.plugins.push(plugin);
    }

    /// Get all plugins
    pub fn plugins(&self) -> &[std::sync::Arc<dyn Plugin>] {
        &self.plugins
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Handle for hot-reloading a pipeline
pub struct HotReloadHandle {
    pipeline: std::sync::Arc<tokio::sync::RwLock<Pipeline>>,
}

impl HotReloadHandle {
    /// Create a new hot-reload handle
    pub fn new(pipeline: Pipeline) -> Self {
        Self {
            pipeline: std::sync::Arc::new(tokio::sync::RwLock::new(pipeline)),
        }
    }

    /// Get the current pipeline (read-only)
    pub async fn get_pipeline(&self) -> Pipeline {
        self.pipeline.read().await.clone()
    }

    /// Update the pipeline
    pub async fn update_pipeline(&self, pipeline: Pipeline) {
        let mut p = self.pipeline.write().await;
        *p = pipeline;
    }

    /// Get the internal Arc for sharing
    pub fn clone_handle(&self) -> std::sync::Arc<tokio::sync::RwLock<Pipeline>> {
        self.pipeline.clone()
    }
}
