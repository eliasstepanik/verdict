use serde::Deserialize;
use std::path::PathBuf;
use verdict::prelude::LlmClient;

/// Application configuration, loaded from file and environment
#[derive(Debug, Deserialize, Default, Clone)]
pub struct AppConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

impl AppConfig {
    /// Load configuration from %APPDATA%\verdict-app\config.toml (Windows)
    /// or ~/.config/verdict-app/config.toml (Linux/Mac) if it exists.
    pub fn load() -> Self {
        let config_path = Self::config_path();
        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(content) => {
                    match toml::from_str::<AppConfig>(&content) {
                        Ok(cfg) => cfg,
                        Err(e) => {
                            eprintln!("⚠  Config file parse error at {}: {}", config_path.display(), e);
                            eprintln!("   Falling back to defaults.");
                            AppConfig::default()
                        }
                    }
                }
                Err(e) => {
                    eprintln!("⚠  Could not read config file at {}: {}", config_path.display(), e);
                    AppConfig::default()
                }
            }
        } else {
            AppConfig::default()
        }
    }

    /// Get the config file path.
    /// Windows: %APPDATA%\verdict-app\config.toml
    /// Linux/Mac: ~/.config/verdict-app/config.toml
    pub fn config_path() -> PathBuf {
        let base = std::env::var("APPDATA")
            .map(PathBuf::from)
            .or_else(|_| {
                std::env::var("HOME").map(|h| {
                    PathBuf::from(h).join(".config")
                })
            })
            .unwrap_or_else(|_| PathBuf::from("."));

        base.join("verdict-app").join("config.toml")
    }

    /// Print a diagnostic summary of where config was loaded from and what's active.
    pub fn print_info(&self) {
        let path = Self::config_path();
        if path.exists() {
            println!("Config file : {}", path.display());
        } else {
            println!("Config file : (not found — expected at {})", path.display());
        }
        println!("api_key     : {}", match &self.api_key {
            Some(k) if !k.is_empty() => {
                let redacted = if k.len() > 8 {
                    format!("{}...{}", &k[..4], &k[k.len()-4..])
                } else {
                    "***".to_string()
                };
                format!("set ({})", redacted)
            }
            _ => "NOT SET — LLM will not work".to_string(),
        });
        println!("base_url    : {}", self.base_url.as_deref().unwrap_or("https://api.openai.com (default)"));
        println!("model       : {}", self.effective_model());
        println!();
        println!("Env vars (used only when config file does not set the value):");
        for var in &["OPENAI_API_KEY", "OPENAI_BASE_URL", "OPENAI_MODEL"] {
            match std::env::var(var) {
                Ok(v) if !v.is_empty() => println!("  {} = {} (present but config file takes priority)", var,
                    if *var == "OPENAI_API_KEY" { "***".to_string() } else { v }),
                _ => println!("  {} = (not set)", var),
            }
        }
    }

    /// Merge environment variables.
    /// Config file values take priority when explicitly set.
    /// Env vars only fill in values that are missing from the config file.
    pub fn merged_with_env(mut self) -> Self {
        if self.api_key.is_none() {
            if let Ok(v) = std::env::var("OPENAI_API_KEY") {
                if !v.is_empty() { self.api_key = Some(v); }
            }
        }
        if self.base_url.is_none() {
            if let Ok(v) = std::env::var("OPENAI_BASE_URL") {
                if !v.is_empty() { self.base_url = Some(v); }
            }
        }
        if self.model.is_none() {
            if let Ok(v) = std::env::var("OPENAI_MODEL") {
                if !v.is_empty() { self.model = Some(v); }
            }
        }
        self
    }

    /// Build an LlmClient from the configuration.
    /// Config file values are used, env vars take precedence (already merged via merged_with_env).
    /// Returns None if no API key is available.
    pub fn build_llm_client(&self) -> Option<LlmClient> {
        let api_key = self.api_key.as_deref().filter(|k| !k.is_empty())?;
        LlmClient::from_env_with_overrides(
            Some(api_key),
            self.base_url.as_deref(),
            self.model.as_deref(),
        ).ok()
    }

    /// Get the effective model name (from config or env or default)
    pub fn effective_model(&self) -> String {
        self.model
            .clone()
            .or_else(|| std::env::var("OPENAI_MODEL").ok())
            .unwrap_or_else(|| "gpt-4o".to_string())
    }

    /// Get the effective system prompt (from config or default).
    /// Injects the current working directory so the LLM knows exactly where "." points.
    pub fn effective_system_prompt(&self) -> String {
        let cwd = std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| ".".to_string());

        self.system_prompt.clone().unwrap_or_else(|| {
            format!(
                "You are a helpful coding assistant with filesystem and shell tools available via function-calling.\n\n\
                Working directory: {cwd}\n\n\
                Use the provided tools (fs_read, fs_write, fs_list, search_files, search_grep, shell_run) to complete tasks. \
                Do not simulate or describe tool calls — call the actual functions. \
                When asked to create a file, call fs_write with the path and full content."
            )
        })
    }
}


