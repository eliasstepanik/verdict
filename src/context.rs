use chrono::{DateTime, Utc};
use serde_json::Value;
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::action::StepOutput;
use crate::agent::{FilesystemPolicy, NetworkPolicy};
use crate::llm::provider::MessageHistory;
use crate::registry::{AgentRegistry, ToolRegistry};
use crate::skills::registry::SkillRegistry;
use crate::toolset::ToolSet;

/// Serializable form of StepContext for persistence and checkpointing.
/// Fields that cannot be serialized (like Arc<LlmClient>) are omitted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerializableStepContext {
    pub agent_name: String,
    pub pipeline_name: String,
    pub step_name: String,
    pub step_id: String,
    
    pub request: Value,
    pub input: Value,  // Variables/state
    pub output: Option<StepOutput>,
    
    pub step_results: HashMap<String, StepResult>,
    
    pub delegation_depth: u32,
    pub parent_agent: Option<String>,
    
    pub active_skills: Vec<String>,
    
    pub trace: PipelineTrace,
    pub budget: BudgetState,
    
    /// Conversation history (serializable)
    pub conversation_history: MessageHistory,
    
    /// Custom metadata for extensions
    pub metadata: Value,
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetState {
    pub remaining_usd: Option<f64>,
    pub llm_calls_used: u32,
    pub tool_calls_used: u32,
    #[serde(skip, default = "default_instant")]
    pub start_time: std::time::Instant,
}

fn default_instant() -> std::time::Instant {
    std::time::Instant::now()
}

impl Default for BudgetState {
    fn default() -> Self {
        Self {
            remaining_usd: None,
            llm_calls_used: 0,
            tool_calls_used: 0,
            start_time: std::time::Instant::now(),
        }
    }
}

/// Context passed to guard, verdict, and action evaluations
#[derive(Clone)]
pub struct StepContext {
    pub agent_name: String,
    pub pipeline_name: String,
    pub step_name: String,

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
            step_name,
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
            trace: self.trace.clone(),
            budget: self.budget.clone(),
            conversation_history: self.conversation_history.clone(),
            metadata: Value::Object(serde_json::Map::new()),
        }
    }

    /// Restore a StepContext from a serializable form.
    /// Non-serializable fields (registries, llm_client, policies) are re-initialized with defaults.
    pub fn from_serializable(
        serializable: SerializableStepContext,
        filesystem_policy: FilesystemPolicy,
    ) -> Self {
        Self {
            agent_name: serializable.agent_name,
            pipeline_name: serializable.pipeline_name,
            step_name: serializable.step_name,
            request: serializable.request,
            input: serializable.input,
            output: serializable.output,
            step_results: serializable.step_results,
            agent_registry: Arc::new(AgentRegistry::new()),
            tool_registry: Arc::new(ToolRegistry::with_builtins()),
            skill_registry: Arc::new(SkillRegistry::new()),
            delegation_depth: serializable.delegation_depth,
            parent_agent: serializable.parent_agent,
            allowed_tools: ToolSet::Full,
            active_skills: serializable.active_skills,
            trace: serializable.trace,
            budget: serializable.budget,
            filesystem_policy,
            network_policy: NetworkPolicy::DenyAll,
            llm_client: None,
            conversation_history: serializable.conversation_history,
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
        self.dir.join(format!("{}_{}.json", safe_pipeline, safe_step))
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
        serde_json::from_slice(&bytes)
            .map_err(|e| ContextStoreError::Serialization(e.to_string()))
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

