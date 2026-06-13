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

        // Resolve the target path. For files that don't exist yet (e.g. write
        // targets) canonicalize fails. In that case canonicalize the parent
        // directory (which must exist) and re-join the filename, so we get the
        // same long-name/UNC form that canonicalize(workspace_root) produces.
        let resolved_path = match std::fs::canonicalize(path) {
            Ok(p) => p,
            Err(_) => {
                // Build an absolute path first.
                let abs = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    self.workspace_root.join(path)
                };
                // Canonicalize the parent if possible to normalise drive
                // letters / 8.3 aliases, then re-attach the file name.
                match (abs.parent(), abs.file_name()) {
                    (Some(parent), Some(name)) => {
                        match std::fs::canonicalize(parent) {
                            Ok(canon_parent) => canon_parent.join(name),
                            Err(_) => abs,
                        }
                    }
                    _ => abs,
                }
            }
        };

        // Resolve the workspace root the same way.
        let resolved_root = match std::fs::canonicalize(&self.workspace_root) {
            Ok(p) => p,
            Err(_) => self.workspace_root.clone(),
        };

        // Normalise both paths to the same form before comparing.  On Windows,
        // canonicalize() returns a \\?\ UNC-prefixed path.  When the target file
        // does not exist the fallback is a plain drive path (C:\...), so strip
        // the \\?\ prefix from the root before the starts_with check.
        let norm_path = strip_verbatim_prefix(&resolved_path);
        let norm_root = strip_verbatim_prefix(&resolved_root);

        // Helper closure: check if a candidate path is within any allowed entry
        // in the supplied list. Each entry is canonicalized/normalised the same
        // way as the target path so comparisons are consistent.
        let within_any = |candidate: &std::path::Path, list: &[PathBuf]| -> bool {
            for entry in list {
                let resolved_entry = match std::fs::canonicalize(entry) {
                    Ok(p) => p,
                    Err(_) => entry.clone(),
                };
                let norm_entry = strip_verbatim_prefix(&resolved_entry);
                if candidate.starts_with(&norm_entry) {
                    return true;
                }
            }
            false
        };

        let in_workspace = norm_path.starts_with(&norm_root)
            || norm_path
                .parent()
                .map(|p| p.starts_with(&norm_root))
                .unwrap_or(false);

        if in_workspace {
            // If read_paths or write_paths are non-empty, the path must be
            // within at least one of those allowed entries. If both lists are
            // empty, fall back to allowing anything within the workspace.
            if self.read_paths.is_empty() && self.write_paths.is_empty() {
                return true;
            }
            if within_any(&norm_path, &self.read_paths)
                || within_any(&norm_path, &self.write_paths)
            {
                return true;
            }
            return false;
        }
        // Also allow if the parent directory is within the workspace (covers the
        // case where the file itself does not yet exist).
        if let Some(parent) = norm_path.parent() {
            parent.starts_with(&norm_root)
        } else {
            false
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

/// Strip the Windows verbatim (`\\?\`) prefix from a path so that plain and
/// UNC-canonicalized paths compare equal with `starts_with`.
/// On non-Windows platforms this is a no-op.
fn strip_verbatim_prefix(path: &std::path::Path) -> std::path::PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        std::path::PathBuf::from(&s[4..])
    } else {
        path.to_path_buf()
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
    #[allow(dead_code)]
    timeout_secs: u64,
}

impl RemoteAgentClient {
    /// Create a new remote agent client with default 30-second timeout
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            timeout_secs: 30,
        }
    }

    /// Create a new remote agent client with custom timeout
    pub fn with_timeout(secs: u64) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(secs))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            timeout_secs: secs,
        }
    }

    /// Execute a step on a remote agent with retry logic and exponential backoff
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

        let mut last_err: Option<Box<dyn std::error::Error + Send + Sync>> = None;

        for attempt in 0u32..3 {
            // Apply exponential backoff on retry (but not on first attempt)
            if attempt > 0 {
                let backoff_secs = 2u64.pow(attempt - 1);
                tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
            }

            match self.client.post(&url).json(&payload).send().await {
                Ok(response) if response.status().is_success() => {
                    // Successful response received
                    match response.json::<serde_json::Value>().await {
                        Ok(result) => return Ok(result),
                        Err(e) => {
                            last_err = Some(Box::new(e));
                            continue;
                        }
                    }
                }
                Ok(response) => {
                    // HTTP error response
                    last_err = Some(Box::new(
                        std::io::Error::new(
                            std::io::ErrorKind::Other,
                            format!("HTTP {}", response.status()),
                        )
                    ));
                }
                Err(e) => {
                    // Network error
                    last_err = Some(Box::new(e));
                }
            }
        }

        // All retries exhausted
        if let Some(err) = last_err {
            if let Some(timeout_err) = err.downcast_ref::<reqwest::Error>() {
                if timeout_err.is_timeout() {
                    return Err(crate::action::RemoteAgentError::Timeout);
                }
            }
            Err(crate::action::RemoteAgentError::NetworkError(err.to_string()))
        } else {
            Err(crate::action::RemoteAgentError::NetworkError(
                "max retries exceeded".to_string(),
            ))
        }
    }
}

impl Default for RemoteAgentClient {
    fn default() -> Self {
        Self::new()
    }
}
