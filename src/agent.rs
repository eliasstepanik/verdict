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

impl FilesystemPolicy {
    /// Check if a path is allowed for access
    pub fn is_path_allowed(&self, path: &std::path::Path) -> bool {
        // Check if path is in forbidden list
        for forbidden in &self.forbidden_paths {
            if path.starts_with(forbidden) {
                return false;
            }
        }

        // Try to canonicalize both paths for comparison
        let canonical_path = match std::fs::canonicalize(path) {
            Ok(p) => p,
            Err(_) => {
                // If we can't canonicalize (file doesn't exist yet), use the path as-is
                // but ensure it's within workspace
                let abs_path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.workspace_root.join(path)
                };
                abs_path
            }
        };

        let canonical_root = match std::fs::canonicalize(&self.workspace_root) {
            Ok(p) => p,
            Err(_) => self.workspace_root.clone(),
        };

        // Check if path is within workspace root
        match canonical_path.canonicalize() {
            Ok(p) => p.starts_with(&canonical_root),
            Err(_) => {
                // If path doesn't exist, check if the parent is within workspace
                if let Some(parent) = canonical_path.parent() {
                    parent.starts_with(&canonical_root)
                } else {
                    false
                }
            }
        }
    }
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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


/// Client for executing steps on a remote agent endpoint
pub struct RemoteAgentClient {
    client: reqwest::Client,
}

impl RemoteAgentClient {
    /// Create a new remote agent client
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    /// Execute a step on a remote agent
    pub async fn execute(
        &self,
        endpoint: &str,
        agent_name: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, crate::action::RemoteAgentError> {
        let url = format!(
            "{}/agents/{}/execute",
            endpoint.trim_end_matches('/'),
            agent_name
        );

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| crate::action::RemoteAgentError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(crate::action::RemoteAgentError::RequestFailed(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            )));
        }

        let result = response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| crate::action::RemoteAgentError::InvalidResponse(e.to_string()))?;

        Ok(result)
    }
}

impl Default for RemoteAgentClient {
    fn default() -> Self {
        Self::new()
    }
}
