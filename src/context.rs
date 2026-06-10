use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

use crate::action::StepOutput;
use crate::agent::{FilesystemPolicy, NetworkPolicy};
use crate::registry::{AgentRegistry, ToolRegistry};
use crate::skills::registry::SkillRegistry;
use crate::toolset::ToolSet;

/// Output from a step action
#[derive(Debug, Clone)]
pub struct StepResult {
    pub step_name: String,
    pub output: StepOutput,
    pub verdict_passed: bool,
    pub error: Option<String>,
}

/// A trace entry for a single step execution
#[derive(Debug, Clone)]
pub struct TraceEntry {
    pub step_name: String,
    pub status: String,
    pub timestamp: DateTime<Utc>,
}

/// Pipeline execution trace
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct BudgetState {
    pub remaining_usd: Option<f64>,
    pub llm_calls_used: u32,
    pub tool_calls_used: u32,
    pub start_time: std::time::Instant,
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
        }
    }

    /// Check if the trace has entries (for TraceAvailable guard)
    pub fn has_trace(&self) -> bool {
        !self.trace.entries.is_empty()
    }
}
