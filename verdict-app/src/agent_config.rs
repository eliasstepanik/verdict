//! verdict-config: Load Verdict Agent definitions from TOML/JSON config files.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use verdict::prelude::*;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error reading {path}: {reason}")]
    Io { path: String, reason: String },
    #[error("Parse error in {path}: {reason}")]
    Parse { path: String, reason: String },
    #[error("Validation error: {0}")]
    Validation(String),
}

/// TOML-representable agent specification
#[derive(Debug, Serialize, Deserialize)]
pub struct AgentSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tools: ToolSetSpec,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub policy: PolicySpec,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PolicySpec {
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_max_delegation_depth")]
    pub max_delegation_depth: u32,
    pub max_cost_usd: Option<f64>,
    #[serde(default)]
    pub allow_self_update: bool,
}

fn default_max_steps() -> u32 {
    100
}
fn default_max_retries() -> u32 {
    3
}
fn default_max_delegation_depth() -> u32 {
    5
}

#[derive(Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolSetSpec {
    None,
    ReadOnly,
    #[default]
    ReadWrite,
    Full,
    Allow(Vec<String>),
    Deny(Vec<String>),
}

impl ToolSetSpec {
    pub fn into_toolset(self) -> ToolSet {
        match self {
            ToolSetSpec::None => ToolSet::None,
            ToolSetSpec::ReadOnly => ToolSet::ReadOnly,
            ToolSetSpec::ReadWrite => ToolSet::ReadWrite,
            ToolSetSpec::Full => ToolSet::Full,
            ToolSetSpec::Allow(v) => ToolSet::Allow(v),
            ToolSetSpec::Deny(v) => ToolSet::Deny(v),
        }
    }
}

impl AgentSpec {
    /// Parse from TOML string
    pub fn from_toml(s: &str) -> Result<Self, ConfigError> {
        toml::from_str(s).map_err(|e| ConfigError::Parse {
            path: "<inline>".into(),
            reason: e.to_string(),
        })
    }

    /// Parse from TOML file
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
        toml::from_str(&content).map_err(|e| ConfigError::Parse {
            path: path.display().to_string(),
            reason: e.to_string(),
        })
    }

    /// Convert to a minimal Verdict Agent (single LLM call pipeline step)
    /// The step uses Guard::None and Verdict::Pass by default.
    pub fn into_agent(self) -> Result<Agent, ConfigError> {
        let tools = self.tools.into_toolset();
        let policy = AgentPolicy {
            max_steps: self.policy.max_steps,
            max_retries: self.policy.max_retries,
            max_delegation_depth: self.policy.max_delegation_depth,
            max_cost_usd: self.policy.max_cost_usd,
            allow_self_update: self.policy.allow_self_update,
            ..Default::default()
        };
        let pipeline = Pipeline {
            name: format!("{}-pipeline", self.name),
            steps: vec![AgentStep {
                name: "respond".into(),
                guard_in: Guard::None,
                action: StepAction::LlmCall {
                    system: format!("You are {}. {}", self.name, self.description),
                    user: "{{input}}".into(),
                    model: None,
                    conversation_id: Some(self.name.clone()),
                    append_to_history: true,
                },
                guard_out: Guard::NonEmptyOutput,
                verdict: Verdict::None,
                tools: tools.clone(),
                injection_protection: InjectionProtection::None,
                output_schema: None,
                dependencies: vec![],
                parallel: false,
                input_processors: vec![],
                output_processors: vec![],
            }],
            on_failure: FailureMode::Abort,
            max_retries: self.policy.max_retries,
        };

        Ok(Agent {
            name: self.name,
            description: self.description,
            pipeline,
            tools,
            skills: SkillSet {
                skills: self.skills,
            },
            policy,
            scorers: Vec::new(),
        })
    }
}

/// Loads multiple agent specs from a directory of TOML files
pub struct AgentLoader {
    pub search_paths: Vec<PathBuf>,
}

impl AgentLoader {
    pub fn new() -> Self {
        AgentLoader {
            search_paths: vec![],
        }
    }

    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.search_paths.push(path);
        self
    }

    /// Load all *.toml files in search_paths, convert to Agents
    pub fn load_all(&self) -> Result<Vec<Agent>, ConfigError> {
        let mut agents = vec![];
        for dir in &self.search_paths {
            let entries = std::fs::read_dir(dir).map_err(|e| ConfigError::Io {
                path: dir.display().to_string(),
                reason: e.to_string(),
            })?;
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) == Some("toml") {
                    let spec = AgentSpec::from_file(&p)?;
                    agents.push(spec.into_agent()?);
                }
            }
        }
        Ok(agents)
    }
}

impl Default for AgentLoader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_toml() {
        let toml = r#"
name = "assistant"
description = "A helpful assistant"
"#;
        let spec = AgentSpec::from_toml(toml).unwrap();
        assert_eq!(spec.name, "assistant");
        let agent = spec.into_agent().unwrap();
        assert_eq!(agent.name, "assistant");
        assert_eq!(agent.pipeline.steps.len(), 1);
    }

    #[test]
    fn test_parse_full_toml() {
        let toml = r#"
name = "coder"
description = "Writes code"
tools = "read_write"
skills = ["rust-debugging"]

[policy]
max_steps = 50
max_retries = 2
max_delegation_depth = 2
max_cost_usd = 2.50
allow_self_update = false
"#;
        let spec = AgentSpec::from_toml(toml).unwrap();
        assert_eq!(spec.name, "coder");
        assert_eq!(spec.skills, vec!["rust-debugging"]);
        assert_eq!(spec.policy.max_steps, 50);
        let agent = spec.into_agent().unwrap();
        assert_eq!(agent.name, "coder");
    }

    #[test]
    fn test_invalid_toml_error() {
        let result = AgentSpec::from_toml("not valid toml {{{{");
        assert!(result.is_err());
    }

    #[test]
    fn test_agent_loader_empty_dir() {
        let tmp = std::env::temp_dir().join("verdict_config_test_empty");
        let _ = std::fs::create_dir_all(&tmp);
        let loader = AgentLoader::new().with_path(tmp.clone());
        let agents = loader.load_all().unwrap();
        assert!(agents.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_agent_loader_with_file() {
        let tmp = std::env::temp_dir().join("verdict_config_test");
        let _ = std::fs::create_dir_all(&tmp);
        std::fs::write(
            tmp.join("test.toml"),
            r#"
name = "test-agent"
description = "Test"
"#,
        )
        .unwrap();
        let loader = AgentLoader::new().with_path(tmp.clone());
        let agents = loader.load_all().unwrap();
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0].name, "test-agent");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_toolset_spec_conversions() {
        let spec = ToolSetSpec::ReadOnly;
        let toolset = spec.into_toolset();
        assert!(matches!(toolset, ToolSet::ReadOnly));

        let spec = ToolSetSpec::Full;
        let toolset = spec.into_toolset();
        assert!(matches!(toolset, ToolSet::Full));

        let spec = ToolSetSpec::Allow(vec!["fs.read".into(), "fs.write".into()]);
        let toolset = spec.into_toolset();
        assert!(matches!(toolset, ToolSet::Allow(_)));
    }
}
