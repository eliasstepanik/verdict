//! verdict-app: Interactive AI assistant built on the Verdict framework.
//!
//! Usage:
//!   verdict-app [chat] [agent-name]   Start interactive chat (default)
//!   verdict-app server                Start agent server on stdio
//!   verdict-app --help                Show this help
//!
//! Configuration:
//!   Set environment variables or create ~/.config/verdict-app/config.toml:
//!     api_key = "sk-..."
//!     base_url = "https://api.openai.com"
//!     model = "gpt-4o"
//!     system_prompt = "You are a helpful assistant."

mod agent;
mod chat;
mod config;
mod memory;
mod server;
mod telemetry;

use config::AppConfig;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let config = AppConfig::load().merged_with_env();

    match args.get(1).map(String::as_str) {
        Some("server") => server::run(config).await,
        Some("config-info") | Some("--config-info") => config.print_info(),
        Some("chat") => {
            let agent_name = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "assistant".to_string());
            chat::run(config, agent_name).await;
        }
        Some("--help") | Some("-h") => {
            println!("verdict-app — interactive AI assistant");
            println!();
            println!("USAGE:");
            println!("  verdict-app [chat] [agent-name]   Start interactive chat (default)");
            println!("  verdict-app server                Start agent server on stdio");
            println!("  verdict-app config-info           Show active config and where it was loaded from");
            println!();
            println!("CONFIG FILE (Windows): %APPDATA%\\verdict-app\\config.toml");
            println!("CONFIG FILE (Linux):   ~/.config/verdict-app/config.toml");
            println!();
            println!("  api_key = \"sk-...\"");
            println!("  base_url = \"https://openrouter.ai/api/v1\"");
            println!("  model = \"anthropic/claude-3.5-sonnet\"");
            println!("  system_prompt = \"You are a helpful assistant.\"");
            println!();
            println!("ENV VARS (override config file):");
            println!("  OPENAI_API_KEY    API key");
            println!("  OPENAI_BASE_URL   Provider base URL");
            println!("  OPENAI_MODEL      Model name");
        }
        None => {
            let agent_name = args
                .get(2)
                .cloned()
                .unwrap_or_else(|| "assistant".to_string());
            chat::run(config, agent_name).await;
        }
        Some(cmd) => {
            eprintln!("Unknown command: {}", cmd);
            eprintln!("Run with --help for usage information.");
            std::process::exit(1);
        }
    }
}
