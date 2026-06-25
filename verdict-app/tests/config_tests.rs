use verdict_app::config::AppConfig;
use std::env;

// ============================================================================
// DEFAULT CONFIG TESTS
// ============================================================================

#[test]
fn test_default_config_has_no_api_key() {
    let config = AppConfig::default();
    assert_eq!(config.api_key, None);
}

#[test]
fn test_default_config_has_no_base_url() {
    let config = AppConfig::default();
    assert_eq!(config.base_url, None);
}

#[test]
fn test_default_config_has_no_model() {
    let config = AppConfig::default();
    assert_eq!(config.model, None);
}

#[test]
fn test_default_config_has_no_system_prompt() {
    let config = AppConfig::default();
    assert_eq!(config.system_prompt, None);
}

#[test]
fn test_default_config_build_llm_client_returns_none() {
    let config = AppConfig::default();
    assert!(config.build_llm_client().is_none());
}

// ============================================================================
// ENVIRONMENT VARIABLE MERGING
// ============================================================================

#[test]
fn test_merged_with_env_picks_up_openai_api_key() {
    env::set_var("OPENAI_API_KEY", "test-key-123");

    let config = AppConfig::default().merged_with_env();

    assert_eq!(config.api_key, Some("test-key-123".to_string()));

    env::remove_var("OPENAI_API_KEY");
}

#[test]
fn test_merged_with_env_ignores_empty_api_key() {
    env::set_var("OPENAI_API_KEY", "");

    let config = AppConfig::default().merged_with_env();

    assert_eq!(config.api_key, None);

    env::remove_var("OPENAI_API_KEY");
}

#[test]
fn test_merged_with_env_picks_up_base_url() {
    env::set_var("OPENAI_BASE_URL", "https://api.example.com");

    let config = AppConfig::default().merged_with_env();

    assert_eq!(config.base_url, Some("https://api.example.com".to_string()));

    env::remove_var("OPENAI_BASE_URL");
}

#[test]
fn test_merged_with_env_picks_up_model() {
    env::set_var("OPENAI_MODEL", "gpt-3.5-turbo");

    let config = AppConfig::default().merged_with_env();

    assert_eq!(config.model, Some("gpt-3.5-turbo".to_string()));

    env::remove_var("OPENAI_MODEL");
}

#[test]
fn test_env_does_not_override_config_file_value() {
    env::set_var("OPENAI_API_KEY", "from-env");

    let config = AppConfig {
        api_key: Some("from-file".to_string()),
        ..Default::default()
    };

    let merged = config.merged_with_env();

    // Config file value should take priority
    assert_eq!(merged.api_key, Some("from-file".to_string()));

    env::remove_var("OPENAI_API_KEY");
}

#[test]
fn test_env_fills_in_missing_values() {
    env::set_var("OPENAI_API_KEY", "env-key");
    env::set_var("OPENAI_MODEL", "env-model");

    let config = AppConfig {
        api_key: None,
        model: None,
        ..Default::default()
    };

    let merged = config.merged_with_env();

    assert_eq!(merged.api_key, Some("env-key".to_string()));
    assert_eq!(merged.model, Some("env-model".to_string()));

    env::remove_var("OPENAI_API_KEY");
    env::remove_var("OPENAI_MODEL");
}

// ============================================================================
// EFFECTIVE MODEL
// ============================================================================

#[test]
fn test_effective_model_defaults_to_gpt4o() {
    let config = AppConfig::default();
    assert_eq!(config.effective_model(), "gpt-4o");
}

#[test]
fn test_effective_model_uses_config_value() {
    let config = AppConfig {
        model: Some("claude-3".to_string()),
        ..Default::default()
    };

    assert_eq!(config.effective_model(), "claude-3");
}

#[test]
fn test_effective_model_prefers_config_over_env() {
    env::set_var("OPENAI_MODEL", "env-model");

    let config = AppConfig {
        model: Some("config-model".to_string()),
        ..Default::default()
    };

    assert_eq!(config.effective_model(), "config-model");

    env::remove_var("OPENAI_MODEL");
}

#[test]
fn test_effective_model_uses_env_when_config_missing() {
    env::set_var("OPENAI_MODEL", "env-model");

    let config = AppConfig::default();

    assert_eq!(config.effective_model(), "env-model");

    env::remove_var("OPENAI_MODEL");
}

// ============================================================================
// EFFECTIVE SYSTEM PROMPT
// ============================================================================

#[test]
fn test_effective_system_prompt_has_default() {
    let config = AppConfig::default();
    let prompt = config.effective_system_prompt();

    assert!(!prompt.is_empty());
    assert!(prompt.contains("assistant") || prompt.contains("helpful"));
}

#[test]
fn test_effective_system_prompt_uses_config_value() {
    let config = AppConfig {
        system_prompt: Some("You are a code expert.".to_string()),
        ..Default::default()
    };

    assert_eq!(
        config.effective_system_prompt(),
        "You are a code expert."
    );
}

// ============================================================================
// CONFIG PATH
// ============================================================================

#[test]
fn test_config_path_contains_verdict_app() {
    let path = AppConfig::config_path();
    let path_str = path.to_string_lossy();

    assert!(path_str.contains("verdict-app"));
    assert!(path_str.contains("config.toml"));
}

#[test]
fn test_config_path_ends_with_config_toml() {
    let path = AppConfig::config_path();
    assert!(path.ends_with("config.toml"));
}

// ============================================================================
// BUILD LLM CLIENT
// ============================================================================

#[test]
fn test_build_llm_client_needs_api_key() {
    let config = AppConfig {
        api_key: Some("".to_string()),
        ..Default::default()
    };

    // Empty string API key should return None
    assert!(config.build_llm_client().is_none());
}

#[test]
fn test_build_llm_client_succeeds_with_api_key() {
    let config = AppConfig {
        api_key: Some("sk-test-key".to_string()),
        ..Default::default()
    };

    // Valid API key should return Some
    let client = config.build_llm_client();
    assert!(client.is_some());
}

#[test]
fn test_build_llm_client_with_all_fields() {
    let config = AppConfig {
        api_key: Some("sk-test".to_string()),
        base_url: Some("https://custom.api.com".to_string()),
        model: Some("gpt-3.5".to_string()),
        system_prompt: Some("You are helpful.".to_string()),
    };

    let client = config.build_llm_client();
    assert!(client.is_some());
}

#[test]
fn test_config_clone() {
    let config = AppConfig {
        api_key: Some("test-key".to_string()),
        base_url: Some("https://api.example.com".to_string()),
        model: Some("gpt-4".to_string()),
        system_prompt: Some("Test prompt".to_string()),
    };

    let cloned = config.clone();

    assert_eq!(cloned.api_key, config.api_key);
    assert_eq!(cloned.base_url, config.base_url);
    assert_eq!(cloned.model, config.model);
    assert_eq!(cloned.system_prompt, config.system_prompt);
}

#[test]
fn test_config_debug_impl() {
    let config = AppConfig {
        api_key: Some("sk-test".to_string()),
        ..Default::default()
    };

    let debug_str = format!("{:?}", config);
    assert!(!debug_str.is_empty());
}

// ============================================================================
// LOAD FROM FILE (integration-style)
// ============================================================================

#[test]
fn test_config_load_returns_default_when_no_file() {
    // AppConfig::load() reads from file if it exists, otherwise returns defaults
    let config = AppConfig::load();

    // Should be a valid config (may have defaults or env-loaded values)
    // Just verify it's a valid AppConfig struct
    let _ = config.effective_model();
    let _ = config.effective_system_prompt();
}

#[test]
fn test_multiple_config_instances_are_independent() {
    let config1 = AppConfig {
        api_key: Some("key1".to_string()),
        ..Default::default()
    };

    let config2 = AppConfig {
        api_key: Some("key2".to_string()),
        ..Default::default()
    };

    assert_ne!(config1.api_key, config2.api_key);
}

#[test]
fn test_config_effective_model_respects_defaults() {
    // Clear env var if set
    env::remove_var("OPENAI_MODEL");

    let config = AppConfig::default();
    assert_eq!(config.effective_model(), "gpt-4o");
}
