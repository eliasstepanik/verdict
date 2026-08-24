use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Verdict project configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VerdictConfig {
    /// Project configuration
    #[serde(default)]
    pub project: ProjectConfig,

    /// Development server configuration
    #[serde(default)]
    pub dev: DevConfig,

    /// Named agent configurations
    #[serde(default)]
    pub agents: HashMap<String, AgentConfig>,

    /// Observability configuration
    #[serde(default)]
    pub observability: ObservabilityConfig,
}

/// Project metadata
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    /// Project name
    #[serde(default)]
    pub name: String,

    /// Project version
    #[serde(default)]
    pub version: String,
}

/// Development server configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DevConfig {
    /// Default agent to run with `verdict dev`
    #[serde(default)]
    pub agent: Option<String>,

    /// Development server port (currently unused)
    #[serde(default)]
    pub port: Option<u16>,

    /// Enable hot-reload
    #[serde(default)]
    pub auto_reload: bool,
}

/// Individual agent configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentConfig {
    /// Cargo binary name
    #[serde(default)]
    pub binary: Option<String>,

    /// Agent description
    #[serde(default)]
    pub description: Option<String>,
}

/// Observability configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ObservabilityConfig {
    /// Enable observability
    #[serde(default)]
    pub enabled: bool,

    /// Observability exporter type ("stdout", "jaeger", "otlp")
    #[serde(default)]
    pub exporter: Option<String>,

    /// Exporter endpoint
    #[serde(default)]
    pub endpoint: Option<String>,
}

/// Configuration error types
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("TOML parse error: {0}")]
    Parse(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

impl VerdictConfig {
    /// Load configuration from a file
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(e))?;
        Self::from_str(&content)
    }

    /// Load configuration from a TOML string
    pub fn from_str(content: &str) -> Result<Self, ConfigError> {
        toml::from_str(content).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Find and load verdict.toml from current directory or parents
    pub fn find_and_load() -> Result<Option<Self>, ConfigError> {
        let mut current = std::env::current_dir()?;
        loop {
            let config_path = current.join("verdict.toml");
            if config_path.exists() {
                return Ok(Some(Self::from_file(&config_path)?));
            }
            if !current.pop() {
                return Ok(None);
            }
        }
    }

    /// Serialize to TOML string
    pub fn to_toml(&self) -> Result<String, ConfigError> {
        toml::to_string_pretty(self).map_err(|e| ConfigError::Parse(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_str() {
        let toml = r#"
[project]
name = "my-agent"
version = "0.1.0"

[dev]
agent = "coder"
port = 8080
auto_reload = true

[agents.coder]
binary = "coder"
description = "Code generation agent"

[observability]
enabled = true
exporter = "stdout"
"#;
        let config = VerdictConfig::from_str(toml).unwrap();
        assert_eq!(config.project.name, "my-agent");
        assert_eq!(config.dev.agent, Some("coder".to_string()));
        assert_eq!(config.dev.port, Some(8080));
        assert!(config.agents.contains_key("coder"));
        assert!(config.observability.enabled);
    }

    #[test]
    fn test_config_to_toml() {
        let config = VerdictConfig {
            project: ProjectConfig {
                name: "test".to_string(),
                version: "1.0.0".to_string(),
            },
            dev: DevConfig {
                agent: Some("coder".to_string()),
                port: Some(8080),
                auto_reload: false,
            },
            agents: HashMap::new(),
            observability: ObservabilityConfig::default(),
        };
        let toml = config.to_toml().unwrap();
        assert!(toml.contains("test"));
        assert!(toml.contains("1.0.0"));
    }
}
