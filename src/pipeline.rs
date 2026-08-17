use serde_json::Value;
use std::sync::Arc;

use crate::action::{StepAction, StepOutput};
use crate::guards::Guard;
use crate::toolset::ToolSet;
use crate::verdict::Verdict;

/// Injection protection strategy for a step
#[derive(Debug, Clone, PartialEq)]
pub enum InjectionProtection {
    /// No protection (Phase 2+: would scan for injection patterns)
    None,

    /// Strict protection (Phase 2+: would apply aggressive filtering)
    Strict,
}

/// Strategy for handling guard processor violations
#[derive(Debug, Clone, Copy)]
pub enum ProcessorStrategy {
    /// Fail the step (default for guard failures)
    Block,

    /// Log warning, continue
    Warn,

    /// Replace output.raw with "[REDACTED]" and continue
    Redact,

    /// Call LLM to rewrite output so guard passes
    Rewrite,
}

/// Processor that wraps guards with named strategies
#[derive(Clone)]
pub struct GuardProcessor {
    /// Processor name
    pub name: String,

    /// The guard being processed
    pub guard: Guard,

    /// Strategy for handling violations
    pub strategy: ProcessorStrategy,

    /// Optional callback for violations
    pub on_violation: Option<Arc<dyn Fn(&str, &ProcessorViolation) + Send + Sync>>,
}

/// Error type for guard processor violations
#[derive(Debug, Clone)]
pub struct ProcessorViolation {
    pub guard: String,
    pub reason: String,
}

impl GuardProcessor {
    /// Create a new guard processor
    pub fn new(name: impl Into<String>, guard: Guard) -> Self {
        Self {
            name: name.into(),
            guard,
            strategy: ProcessorStrategy::Block,
            on_violation: None,
        }
    }

    /// Set the violation strategy
    pub fn with_strategy(mut self, strategy: ProcessorStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    /// Set the violation callback
    pub fn with_on_violation(
        mut self,
        f: impl Fn(&str, &ProcessorViolation) + Send + Sync + 'static,
    ) -> Self {
        self.on_violation = Some(Arc::new(f));
        self
    }
}

impl std::fmt::Debug for GuardProcessor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardProcessor")
            .field("name", &self.name)
            .field("guard", &self.guard)
            .field("strategy", &self.strategy)
            .field("on_violation", &"<callback>")
            .finish()
    }
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

    /// Input processors (guards + strategies) applied before the action
    pub input_processors: Vec<GuardProcessor>,

    /// Output processors (guards + strategies) applied after the action
    pub output_processors: Vec<GuardProcessor>,
}

impl Default for AgentStep {
    fn default() -> Self {
        Self {
            name: "unnamed".into(),
            guard_in: Guard::None,
            action: StepAction::Custom(Arc::new(|_| {
                Ok(StepOutput {
                    raw: String::new(),
                    parsed: None,
                })
            })),
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::Full,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
            input_processors: Vec::new(),
            output_processors: Vec::new(),
        }
    }
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

/// Fluent builder for constructing pipelines
pub struct PipelineBuilder {
    name: String,
    steps: Vec<AgentStep>,
    on_failure: FailureMode,
    max_retries: u32,
    step_counter: usize,
}

impl PipelineBuilder {
    /// Create a new pipeline builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            steps: Vec::new(),
            on_failure: FailureMode::Abort,
            max_retries: 3,
            step_counter: 0,
        }
    }

    /// Add a single sequential step
    pub fn then(mut self, step: AgentStep) -> Self {
        let mut step = step;
        if step.parallel {
            step.parallel = false;
        }
        self.steps.push(step);
        self
    }

    /// Add multiple steps to execute in parallel
    pub fn parallel(mut self, steps: Vec<AgentStep>) -> Self {
        for mut step in steps {
            step.parallel = true;
            self.steps.push(step);
        }
        self
    }

    /// Add a branch step (conditional execution)
    pub fn branch(
        mut self,
        condition: &str,
        if_true: AgentStep,
        if_false: Option<AgentStep>,
    ) -> Self {
        let action = StepAction::Branch {
            condition: condition.to_string(),
            if_true: Box::new(if_true.action),
            if_false: if_false.map(|s| Box::new(s.action)),
        };
        let step_name = format!("branch_{}", self.step_counter);
        self.step_counter += 1;
        let step = AgentStep {
            name: step_name,
            action,
            guard_in: Guard::None,
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
            input_processors: Vec::new(),
            output_processors: Vec::new(),
        };
        self.steps.push(step);
        self
    }

    /// Add a forEach loop step
    pub fn foreach(
        mut self,
        _input_array_key: impl Into<String>,
        body: StepAction,
        _concurrency: usize,
    ) -> Self {
        let step_name = format!("foreach_{}", self.step_counter);
        self.step_counter += 1;
        let action = StepAction::LoopUntil {
            body: Box::new(body),
            condition: Guard::None,
            max_iterations: 1000,
            on_iteration_failure: crate::action::IterationFailureMode::Retry,
        };
        let step = AgentStep {
            name: step_name,
            action,
            guard_in: Guard::None,
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
            input_processors: Vec::new(),
            output_processors: Vec::new(),
        };
        self.steps.push(step);
        self
    }

    /// Add a sleep step
    pub fn sleep(mut self, duration_ms: u64) -> Self {
        let step_name = format!("sleep_{}", self.step_counter);
        self.step_counter += 1;
        let action = StepAction::Custom(Arc::new(move |_ctx| {
            std::thread::sleep(std::time::Duration::from_millis(duration_ms));
            Ok(crate::action::StepOutput {
                raw: format!("slept for {}ms", duration_ms),
                parsed: None,
            })
        }));
        let step = AgentStep {
            name: step_name,
            action,
            guard_in: Guard::None,
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
            input_processors: Vec::new(),
            output_processors: Vec::new(),
        };
        self.steps.push(step);
        self
    }

    /// Add a sleep_until step
    pub fn sleep_until(mut self, _timestamp: chrono::DateTime<chrono::Utc>) -> Self {
        let step_name = format!("sleep_until_{}", self.step_counter);
        self.step_counter += 1;
        let action = StepAction::Custom(Arc::new(move |_ctx| {
            Ok(crate::action::StepOutput {
                raw: "slept until timestamp".to_string(),
                parsed: None,
            })
        }));
        let step = AgentStep {
            name: step_name,
            action,
            guard_in: Guard::None,
            guard_out: Guard::None,
            verdict: Verdict::None,
            tools: ToolSet::None,
            injection_protection: InjectionProtection::None,
            output_schema: None,
            dependencies: Vec::new(),
            parallel: false,
            input_processors: Vec::new(),
            output_processors: Vec::new(),
        };
        self.steps.push(step);
        self
    }

    /// Set the failure mode
    pub fn on_failure(mut self, mode: FailureMode) -> Self {
        self.on_failure = mode;
        self
    }

    /// Set max retries
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Build the final pipeline
    pub fn build(self) -> Pipeline {
        Pipeline {
            name: self.name,
            steps: self.steps,
            on_failure: self.on_failure,
            max_retries: self.max_retries,
        }
    }
}

/// Builder for constructing AgentSteps fluently
pub struct AgentStepBuilder {
    step: AgentStep,
}

impl AgentStepBuilder {
    /// Create a new step builder
    pub fn new(name: impl Into<String>, action: StepAction) -> Self {
        Self {
            step: AgentStep {
                name: name.into(),
                action,
                guard_in: Guard::None,
                guard_out: Guard::None,
                verdict: Verdict::None,
                tools: ToolSet::Full,
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: Vec::new(),
                parallel: false,
                input_processors: Vec::new(),
                output_processors: Vec::new(),
            },
        }
    }

    /// Set the input guard
    pub fn guard_in(mut self, guard: Guard) -> Self {
        self.step.guard_in = guard;
        self
    }

    /// Set the output guard
    pub fn guard_out(mut self, guard: Guard) -> Self {
        self.step.guard_out = guard;
        self
    }

    /// Set the verdict
    pub fn verdict(mut self, verdict: Verdict) -> Self {
        self.step.verdict = verdict;
        self
    }

    /// Set the tool scope
    pub fn tools(mut self, tools: ToolSet) -> Self {
        self.step.tools = tools;
        self
    }

    /// Set injection protection
    pub fn injection_protection(mut self, protection: InjectionProtection) -> Self {
        self.step.injection_protection = protection;
        self
    }

    /// Set output schema
    pub fn output_schema(mut self, schema: Value) -> Self {
        self.step.output_schema = Some(schema);
        self
    }

    /// Add a dependency on another step
    pub fn depends_on(mut self, step_name: impl Into<String>) -> Self {
        self.step.dependencies.push(step_name.into());
        self
    }

    /// Set parallel execution flag
    pub fn parallel(mut self, parallel: bool) -> Self {
        self.step.parallel = parallel;
        self
    }

    /// Add an input processor
    pub fn input_processor(mut self, processor: GuardProcessor) -> Self {
        self.step.input_processors.push(processor);
        self
    }

    /// Add an output processor
    pub fn output_processor(mut self, processor: GuardProcessor) -> Self {
        self.step.output_processors.push(processor);
        self
    }

    /// Build the final step
    pub fn build(self) -> AgentStep {
        self.step
    }
}

impl AgentStep {
    /// Create a builder for this step
    pub fn builder(name: impl Into<String>, action: StepAction) -> AgentStepBuilder {
        AgentStepBuilder::new(name, action)
    }
}
