use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::action::StepOutput;
use crate::agent::{FilesystemPolicy, NetworkPolicy};
use crate::cancel::CancellationToken;
use crate::llm::provider::MessageHistory;
use crate::registry::{AgentRegistry, ToolRegistry};
use crate::skills::registry::SkillRegistry;
use crate::toolset::ToolSet;

/// Per-request dynamic configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestContext {
    /// Dynamic values set per request
    values: HashMap<String, Value>,
    /// Dynamic toolsets per agent (Phase D2)
    #[serde(skip)]
    toolsets: HashMap<String, Arc<ToolRegistry>>,
}

impl Default for RequestContext {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            toolsets: HashMap::new(),
        }
    }
}

impl RequestContext {
    /// Create a new empty request context
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            toolsets: HashMap::new(),
        }
    }

    /// Set a value (builder style)
    pub fn set(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        if let Ok(json_value) = serde_json::to_value(value) {
            self.values.insert(key.into(), json_value);
        }
        self
    }

    /// Get a value
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Get a value as a string
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.values.get(key).and_then(|v| v.as_str())
    }

    /// Get a value as a boolean
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.values.get(key).and_then(|v| v.as_bool())
    }

    /// Get a value as a number
    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.values.get(key).and_then(|v| v.as_f64())
    }

    /// Insert a value directly
    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.values.insert(key.into(), value);
    }

    /// Merge another RequestContext into this one
    pub fn merge(&mut self, other: &RequestContext) {
        for (k, v) in &other.values {
            self.values.insert(k.clone(), v.clone());
        }
    }

    /// Set a per-agent toolset override (Phase D2)
    pub fn with_toolset(
        &mut self,
        agent_name: impl Into<String>,
        registry: Arc<ToolRegistry>,
    ) -> &mut Self {
        self.toolsets.insert(agent_name.into(), registry);
        self
    }

    /// Get a per-agent toolset override (Phase D2)
    pub fn get_toolset(&self, agent_name: &str) -> Option<Arc<ToolRegistry>> {
        self.toolsets.get(agent_name).cloned()
    }
}

/// Serializable form of StepContext for persistence and checkpointing.
/// Fields that cannot be serialized (like Arc<LlmClient>) are omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableStepContext {
    pub agent_name: String,
    pub pipeline_name: String,
    pub step_name: String,
    pub step_id: String,

    pub request: Value,
    pub input: Value, // Variables/state
    pub output: Option<StepOutput>,

    pub step_results: HashMap<String, StepResult>,

    pub delegation_depth: u32,
    pub parent_agent: Option<String>,

    pub active_skills: Vec<String>,
    pub allowed_tools: ToolSet,

    pub trace: PipelineTrace,
    pub budget: BudgetState,

    /// Conversation history (serializable)
    pub conversation_history: MessageHistory,

    /// Filesystem policy for restored context
    pub filesystem_policy: FilesystemPolicy,

    /// Network policy for restored context
    pub network_policy: NetworkPolicy,

    /// Custom metadata for extensions
    pub metadata: Value,

    /// Per-request dynamic configuration (Phase B)
    pub request_context: RequestContext,
}

/// Output from a step action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_name: String,
    pub output: StepOutput,
    pub verdict_passed: bool,
    pub error: Option<String>,
}

/// A trace entry for a single step execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEntry {
    pub step_name: String,
    pub status: String,
    pub timestamp: DateTime<Utc>,
}

/// Pipeline execution trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineTrace {
    pub entries: Vec<TraceEntry>,
}

impl PipelineTrace {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn append(&mut self, entry: TraceEntry) {
        self.entries.push(entry);
    }
}

impl Default for PipelineTrace {
    fn default() -> Self {
        Self::new()
    }
}

/// Budget/cost tracking state
#[derive(Debug, Serialize)]
pub struct BudgetState {
    pub spent_usd: f64,
    pub remaining_usd: Option<f64>,
    pub llm_calls_used: u32,
    pub tool_calls_used: u32,
    #[serde(skip)]
    pub start_time: std::time::Instant,
    /// Elapsed time since checkpoint load (in seconds), if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_secs_since_load: Option<f64>,
}

impl<'de> serde::Deserialize<'de> for BudgetState {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct BudgetStateDe {
            spent_usd: f64,
            remaining_usd: Option<f64>,
            llm_calls_used: u32,
            tool_calls_used: u32,
            elapsed_secs_since_load: Option<f64>,
        }

        let de = BudgetStateDe::deserialize(deserializer)?;
        Ok(BudgetState {
            spent_usd: de.spent_usd,
            remaining_usd: de.remaining_usd,
            llm_calls_used: de.llm_calls_used,
            tool_calls_used: de.tool_calls_used,
            start_time: std::time::Instant::now(),
            elapsed_secs_since_load: de.elapsed_secs_since_load,
        })
    }
}

impl Clone for BudgetState {
    fn clone(&self) -> Self {
        // When cloning for checkpoint/snapshot, preserve elapsed time but reset start_time to now
        // This prevents the resumed context from having a fresh budget clock
        let current_elapsed = self.start_time.elapsed().as_secs_f64();
        let total_elapsed = current_elapsed + self.elapsed_secs_since_load.unwrap_or(0.0);
        Self {
            spent_usd: self.spent_usd,
            remaining_usd: self.remaining_usd,
            llm_calls_used: self.llm_calls_used,
            tool_calls_used: self.tool_calls_used,
            start_time: std::time::Instant::now(),
            elapsed_secs_since_load: Some(total_elapsed),
        }
    }
}

impl Default for BudgetState {
    fn default() -> Self {
        Self {
            spent_usd: 0.0,
            remaining_usd: None,
            llm_calls_used: 0,
            tool_calls_used: 0,
            start_time: std::time::Instant::now(),
            elapsed_secs_since_load: None,
        }
    }
}

/// Context passed to guard, verdict, and action evaluations
#[derive(Clone, Debug)]

pub struct StepContext {
    pub agent_name: String,
    pub pipeline_name: String,
    pub step_name: String,
    pub step_id: String,

    pub request: Value,
    pub input: Value,
    pub output: Option<StepOutput>,

    pub step_results: HashMap<String, StepResult>,

    pub agent_registry: Arc<AgentRegistry>,
    pub tool_registry: Arc<ToolRegistry>,
    pub skill_registry: Arc<SkillRegistry>,

    pub delegation_depth: u32,
    pub parent_agent: Option<String>,

    pub allowed_tools: ToolSet,
    pub active_skills: Vec<String>,

    pub trace: PipelineTrace,
    pub budget: BudgetState,
    pub filesystem_policy: FilesystemPolicy,
    pub network_policy: NetworkPolicy,

    /// Optional LLM client for verdict evaluation (e.g., Verdict::LlmJudge)
    pub llm_client: Option<Arc<crate::llm::LlmClient>>,

    /// Conversation history for multi-turn LLM interactions
    pub conversation_history: MessageHistory,

    /// Tools that were actually called during this step execution
    pub tools_used: Vec<String>,

    /// Optional session context (set when running inside a SessionRunner) — Phase 13
    pub session_meta: Option<crate::session::SessionMeta>,

    /// Cancellation token for interrupting pipeline execution (Phase 14)
    pub cancellation_token: CancellationToken,

    /// Per-request dynamic configuration (Phase B)
    pub request_context: RequestContext,

    /// Multi-tier memory store (Phase C1)
    pub memory: Option<Arc<dyn crate::memory::MemoryStore>>,
}

impl StepContext {
    pub fn new(
        agent_name: String,
        pipeline_name: String,
        step_name: String,
        request: Value,
        filesystem_policy: FilesystemPolicy,
    ) -> Self {
        Self {
            agent_name,
            pipeline_name,
            step_name: step_name.clone(),
            step_id: step_name,

            request,
            input: Value::Null,
            output: None,
            step_results: HashMap::new(),
            agent_registry: Arc::new(AgentRegistry::new()),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            skill_registry: Arc::new(SkillRegistry::new()),
            delegation_depth: 0,
            parent_agent: None,
            allowed_tools: ToolSet::Full,
            active_skills: vec![],
            trace: PipelineTrace::new(),
            budget: BudgetState::default(),
            filesystem_policy,
            network_policy: NetworkPolicy::DenyAll,
            llm_client: None,
            conversation_history: MessageHistory::new(),
            tools_used: vec![],
            session_meta: None,
            cancellation_token: CancellationToken::new(),
            request_context: RequestContext::new(),
            memory: None,
        }
    }

    /// Check if the trace has entries (for TraceAvailable guard)
    pub fn has_trace(&self) -> bool {
        !self.trace.entries.is_empty()
    }

    /// Convert to a serializable form for checkpointing/persistence
    pub fn to_serializable(&self, step_id: String) -> SerializableStepContext {
        SerializableStepContext {
            agent_name: self.agent_name.clone(),
            pipeline_name: self.pipeline_name.clone(),
            step_name: self.step_name.clone(),
            step_id,
            request: self.request.clone(),
            input: self.input.clone(),
            output: self.output.clone(),
            step_results: self.step_results.clone(),
            delegation_depth: self.delegation_depth,
            parent_agent: self.parent_agent.clone(),
            active_skills: self.active_skills.clone(),
            allowed_tools: self.allowed_tools.clone(),
            trace: self.trace.clone(),
            budget: self.budget.clone(),
            conversation_history: self.conversation_history.clone(),
            filesystem_policy: self.filesystem_policy.clone(),
            network_policy: self.network_policy.clone(),

            metadata: Value::Object(serde_json::Map::new()),
            request_context: self.request_context.clone(),
        }
    }
    /// Restore a StepContext from a serializable form.
    ///
    /// # Non-serializable Field Restoration
    ///
    /// The following fields cannot be serialized and are re-initialized:
    ///
    /// - `llm_client`: Defaults to None. If verdict evaluation (Verdict::LlmJudge) or
    ///   semantic guards are needed, the caller MUST re-inject via PipelineRunner.
    ///
    /// All registries (agent, tool, skill) are fresh instances. Caller may need to
    /// pass these from the original context or runner.
    ///
    /// Filesystem and network policies ARE restored from the serialized context,
    /// ensuring security policies are preserved across checkpoint/resume cycles.
    pub fn from_serializable(serializable: SerializableStepContext) -> Self {
        Self {
            agent_name: serializable.agent_name,
            pipeline_name: serializable.pipeline_name,
            step_name: serializable.step_name,
            step_id: serializable.step_id,

            request: serializable.request,
            input: serializable.input,
            output: serializable.output,
            step_results: serializable.step_results,
            agent_registry: Arc::new(AgentRegistry::new()),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            skill_registry: Arc::new(SkillRegistry::new()),
            delegation_depth: serializable.delegation_depth,
            parent_agent: serializable.parent_agent,
            allowed_tools: serializable.allowed_tools,
            active_skills: serializable.active_skills,
            trace: serializable.trace,
            budget: serializable.budget,
            filesystem_policy: serializable.filesystem_policy,
            network_policy: serializable.network_policy,
            llm_client: None,
            conversation_history: serializable.conversation_history,
            tools_used: vec![],
            session_meta: None,
            cancellation_token: CancellationToken::new(),
            request_context: serializable.request_context,
            memory: None,
        }
    }
}

/// Error type for ContextStore operations.
#[derive(Debug, thiserror::Error)]
pub enum ContextStoreError {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("snapshot not found: {0}")]
    NotFound(String),
}

/// Persists and retrieves StepContext snapshots to/from disk as JSON.
/// Each snapshot is stored as `{dir}/{pipeline_name}_{step_name}.json`.
pub struct ContextStore {
    dir: std::path::PathBuf,
}

impl ContextStore {
    /// Create a new ContextStore rooted at `dir`.
    pub fn new(dir: std::path::PathBuf) -> Self {
        Self { dir }
    }

    fn snapshot_path(&self, pipeline_name: &str, step_name: &str) -> std::path::PathBuf {
        let safe_pipeline = pipeline_name.replace(['/', '\\', ' ', ':'], "_");
        let safe_step = step_name.replace(['/', '\\', ' ', ':'], "_");
        self.dir
            .join(format!("{}_{}.json", safe_pipeline, safe_step))
    }

    /// Save a StepContext snapshot to disk.
    pub async fn save(&self, ctx: &StepContext) -> Result<(), ContextStoreError> {
        tokio::fs::create_dir_all(&self.dir)
            .await
            .map_err(|e| ContextStoreError::Io(e.to_string()))?;
        let serializable = ctx.to_serializable(ctx.step_name.clone());
        let json = serde_json::to_string_pretty(&serializable)
            .map_err(|e| ContextStoreError::Serialization(e.to_string()))?;
        let path = self.snapshot_path(&ctx.pipeline_name, &ctx.step_name);
        tokio::fs::write(&path, json)
            .await
            .map_err(|e| ContextStoreError::Io(e.to_string()))
    }

    /// Load a saved snapshot by pipeline and step name.
    pub async fn load(
        &self,
        pipeline_name: &str,
        step_name: &str,
    ) -> Result<SerializableStepContext, ContextStoreError> {
        let path = self.snapshot_path(pipeline_name, step_name);
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ContextStoreError::NotFound(path.display().to_string())
            } else {
                ContextStoreError::Io(e.to_string())
            }
        })?;
        serde_json::from_slice(&bytes).map_err(|e| ContextStoreError::Serialization(e.to_string()))
    }

    /// List all snapshot filenames for a given pipeline.
    pub async fn list_snapshots(
        &self,
        pipeline_name: &str,
    ) -> Result<Vec<String>, ContextStoreError> {
        let safe_pipeline = pipeline_name.replace(['/', '\\', ' ', ':'], "_");
        let prefix = format!("{}_", safe_pipeline);
        let mut entries = tokio::fs::read_dir(&self.dir)
            .await
            .map_err(|e| ContextStoreError::Io(e.to_string()))?;
        let mut names = Vec::new();
        loop {
            match entries.next_entry().await {
                Ok(Some(entry)) => {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with(&prefix) && name.ends_with(".json") {
                        names.push(name);
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(ContextStoreError::Io(e.to_string())),
            }
        }
        Ok(names)
    }

    /// Delete a saved snapshot.
    pub async fn delete(
        &self,
        pipeline_name: &str,
        step_name: &str,
    ) -> Result<(), ContextStoreError> {
        let path = self.snapshot_path(pipeline_name, step_name);
        tokio::fs::remove_file(&path).await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ContextStoreError::NotFound(path.display().to_string())
            } else {
                ContextStoreError::Io(e.to_string())
            }
        })
    }
}
