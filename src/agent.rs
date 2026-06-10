use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::pipeline::Pipeline;
use crate::skills::skill::SkillSet;
use crate::toolset::ToolSet;

/// Workspace isolation strategy for task execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkspaceIsolation {
    /// No isolation; share the default workspace
    None,

    /// Create a fresh temp directory per task run
    TempDir,

    /// Use an explicit sandboxed directory
    Sandboxed(PathBuf),
}

/// Filesystem access policy for agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesystemPolicy {
    /// Root workspace directory
    pub workspace_root: PathBuf,

    /// Paths allowed for reading
    pub read_paths: Vec<PathBuf>,

    /// Paths allowed for writing
    pub write_paths: Vec<PathBuf>,

    /// Paths forbidden for any access
    pub forbidden_paths: Vec<PathBuf>,

    /// Workspace isolation mode
    pub workspace_isolation: WorkspaceIsolation,
}

impl Default for FilesystemPolicy {
    fn default() -> Self {
        Self {
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            read_paths: vec![],
            write_paths: vec![],
            forbidden_paths: vec![],
            workspace_isolation: WorkspaceIsolation::None,
        }
    }
}

/// Network access policy for agents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkPolicy {
    /// No network access allowed
    DenyAll,

    /// Only specified domains/IPs allowed
    AllowList(Vec<String>),

    /// All network access allowed
    AllowAll,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        NetworkPolicy::DenyAll
    }
}

/// Runtime policy constraints for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPolicy {
    /// Maximum number of sequential steps
    pub max_steps: u32,

    /// Maximum retries per step
    pub max_retries: u32,

    /// Maximum delegation nesting depth
    pub max_delegation_depth: u32,

    /// Maximum cost in USD
    pub max_cost_usd: Option<f64>,

    /// Maximum runtime in seconds
    pub max_runtime_seconds: Option<u64>,

    /// Whether this agent can self-update
    pub allow_self_update: bool,

    /// Whether self-updates require human approval
    pub require_approval_for_self_update: bool,

    /// List of agents this agent can delegate to
    pub allowed_agents: Vec<String>,

    /// Tools available to this agent
    pub allowed_tools: ToolSet,

    /// Skills available to this agent
    pub allowed_skills: Vec<String>,

    /// Network access policy
    pub network_policy: NetworkPolicy,

    /// Filesystem access policy
    pub filesystem_policy: FilesystemPolicy,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            max_steps: 100,
            max_retries: 3,
            max_delegation_depth: 5,
            max_cost_usd: Some(10.0),
            max_runtime_seconds: Some(3600),
            allow_self_update: false,
            require_approval_for_self_update: true,
            allowed_agents: vec![],
            allowed_tools: ToolSet::None,
            allowed_skills: vec![],
            network_policy: NetworkPolicy::DenyAll,
            filesystem_policy: FilesystemPolicy::default(),
        }
    }
}

/// A versioned agent snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentVersion {
    pub agent_name: String,
    pub version: String,
    pub parent_version: Option<String>,
    pub created_at: DateTime<Utc>,
    pub change_summary: String,
    pub git_commit: Option<String>,
    pub evaluation_score: Option<f64>,
}

/// An agent: a named pipeline with tools, skills, and policy
#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub description: String,
    pub pipeline: Pipeline,
    pub tools: ToolSet,
    pub skills: SkillSet,
    pub policy: AgentPolicy,
}
