use crate::agent::{build_assistant_agent, build_echo_agent, build_improve_pipeline};
use crate::config::AppConfig;
use crate::telemetry;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{warn, info, error};
use verdict::prelude::*;

/// Run the interactive chat REPL
pub async fn run(config: AppConfig, agent_name: String) {
    // Check if LLM is available
    let has_llm = config.build_llm_client().is_some();

    if has_llm {
        println!("✓ LLM configured: model={}", config.effective_model());
        run_chat_with_llm(config, agent_name).await;
    } else {
        warn!("no API key found; set OPENAI_API_KEY or add config to ~/.config/verdict-app/config.toml");
        info!("falling back to echo mode");
        run_echo_mode(agent_name).await;
    }
}

/// Interactive chat with real LLM
async fn run_chat_with_llm(config: AppConfig, agent_name: String) {
    let llm_client = match config.build_llm_client() {
        Some(c) => Arc::new(c),
        None => return,
    };

    // Build registries
    let tool_registry = ToolRegistry::with_builtins();
    let skill_registry = SkillRegistry::with_builtins();

    // Build agent + registry
    let agent = build_assistant_agent(&config, &agent_name);
    let mut agent_registry = AgentRegistry::new();
    agent_registry.register(agent);

    // Register reflector agent for delegation support
    agent_registry.register(reflector_agent());

    // Create runner with all registries
    let mut runner =
        PipelineRunner::with_registries(Arc::new(tool_registry), Arc::new(agent_registry));
    runner = runner.with_llm_client(Arc::clone(&llm_client));
    runner.skill_registry = Arc::new(skill_registry.clone());

    let runner = Arc::new(Mutex::new(runner));
    let runner_for_improve = Arc::clone(&runner);
    let session_runner = SessionRunner::new(Arc::clone(&runner));

    // Track active skill for the session
    let mut active_skill: Option<String> = None;

    // Create session
    let session_id = match session_runner
        .new_session(&agent_name, SessionPolicy::default())
        .await
    {
        Ok(id) => id,
        Err(e) => {
            error!(error = %e, "failed to create session");
            return;
        }
    };

    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    println!("verdict-app — Interactive Chat");
    println!("Working directory: {}", cwd);
    println!("Commands: /tools, /skills, /skill, /improve, /help, /quit");
    println!();

    loop {
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush().ok();

        let line = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            match std::io::stdin().read_line(&mut buf) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(buf),
            }
        })
        .await
        .unwrap_or(None);

        let line = match line {
            Some(l) => l,
            None => break,
        };

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        match trimmed.as_str() {
            "/quit" | "/exit" => break,
            "/help" => {
                println!();
                println!("Commands:");
                println!("  /tools         List available tools the LLM can use");
                println!("  /skills        List available skills");
                println!("  /skill <name>  Activate a skill for the next turn");
                println!("  /skill         Show currently active skill");
                println!("  /improve       Run self-improvement analysis");
                println!("  /quit          Exit");
                println!();
                continue;
            }
            "/tools" => {
                println!();
                println!("Available tools:");
                println!("  fs.read          Read a file");
                println!("  fs.list          List directory contents");
                println!("  fs.write         Write a file");
                println!("  search.files     Glob file search");
                println!("  search.grep      Regex grep across files");
                println!("  shell.run        Run a shell command");
                println!("  shell.cargo_check  Run cargo check");
                println!("  shell.cargo_test   Run cargo test");
                println!();
                continue;
            }
            "/skills" => {
                println!();
                println!("Available skills (use /skill <name> to activate):");
                for name in skill_registry.list() {
                    if let Some(s) = skill_registry.get(&name) {
                        println!("  {:20} {}", name, s.description);
                    }
                }
                println!();
                continue;
            }
            "/skill" => {
                match &active_skill {
                    Some(s) => println!("[active skill: {}]", s),
                    None => println!("[no skill active]"),
                }
                println!();
                continue;
            }
            line if line.starts_with("/skill ") => {
                let name = line.trim_start_matches("/skill ").trim();
                if skill_registry.get(name).is_some() {
                    active_skill = Some(name.to_string());
                    println!("[skill '{}' activated for next turn]", name);
                } else {
                    println!("[skill '{}' not found — use /skills to list]", name);
                }
                println!();
                continue;
            }
            "/improve" => {
                do_self_improve(&runner_for_improve, &session_runner, &session_id).await;
                continue;
            }
            _ => {}
        }

        // If a skill is active, inject its instructions as a system context block.
        // The framework's UseSkill action isn't available mid-session without rebuilding
        // the agent, so we inject instructions as a clearly-delimited context prefix.
        // Clone name first to release the borrow before reassigning active_skill.
        let text = if let Some(skill_name) = active_skill.clone() {
            if let Some(skill) = skill_registry.get(&skill_name) {
                active_skill = None; // consume — one turn only
                                     // Delimiter keeps skill instructions visually separated from user request
                format!(
                    "<skill name=\"{}\">\n{}\n</skill>\n\n{}",
                    skill_name,
                    skill.instructions.trim(),
                    trimmed
                )
            } else {
                trimmed.clone()
            }
        } else {
            trimmed.clone()
        };

        let turn = UserTurn {
            content: TurnContent { text },
            attachments: vec![],
            interrupt_previous: false,
        };

        match session_runner.turn(&session_id, turn).await {
            Ok(TurnResult::Completed { output, .. }) => {
                println!();
                println!("assistant: {}", output);
                println!();

                // Check if compaction is needed.
                // Token counting in session.rs uses output.len()/4 (byte estimate), so
                // we use message count as the reliable trigger instead: compact after 40 messages.
                maybe_compact_history(
                    &session_runner,
                    &session_id,
                    &llm_client,
                    40, // message count threshold (20 turns = 40 messages)
                    5,  // keep last 5 user+assistant pairs (10 messages)
                )
                .await;
            }
            Ok(TurnResult::Error(msg)) => {
                println!();
                println!("[error: {}]", msg);
                println!();
            }
            Ok(TurnResult::Cancelled { partial_output, .. }) => {
                println!();
                if partial_output.is_empty() {
                    println!("[cancelled]");
                } else {
                    println!("[cancelled: {}]", partial_output);
                }
                println!();
            }
            Ok(TurnResult::GuardFailed { reason, .. }) => {
                println!();
                println!("[guard blocked: {}]", reason);
                println!();
            }
            Ok(TurnResult::AwaitingInput { prompt }) => {
                println!();
                println!("[awaiting: {}]", prompt);
                println!();
            }
            Err(e) => {
                println!();
                println!("[session error: {}]", e);
                println!();
                break;
            }
        }
    }

    session_runner.close(&session_id).await.ok();
    println!();
    println!("Goodbye.");
}

/// Compact conversation history when message count exceeds the threshold.
/// Uses message count (reliable) rather than token estimates (inaccurate in session.rs).
async fn maybe_compact_history(
    session_runner: &SessionRunner,
    session_id: &SessionId,
    llm_client: &Arc<LlmClient>,
    message_count_threshold: usize,
    keep_recent: usize,
) {
    // Fetch full history to get accurate message count
    let history = match session_runner.get_history(session_id).await {
        Ok(h) => h,
        Err(_) => return,
    };

    let total = history.messages.len();

    // Only compact when we have enough messages to make it worthwhile
    if total < message_count_threshold || total <= keep_recent * 2 {
        return;
    }

    let to_summarize = &history.messages[..total - keep_recent * 2];
    if to_summarize.is_empty() {
        return;
    }

    println!(
        "\n[compaction] {} messages in history — summarizing {} old messages...",
        total,
        to_summarize.len()
    );

    let history_text: String = to_summarize
        .iter()
        .map(|m| format!("{:?}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let req = LlmRequest {
        system: "You are a context summarizer. Produce a dense factual summary of this \
                 conversation excerpt. Preserve all key decisions, facts, code, and context \
                 needed to continue the conversation. Be concise but complete."
            .to_string(),
        user: history_text,
        model: String::new(), // use LlmClient default
        max_tokens: Some(512),
        history: None,
        temperature: Some(0.3),
        tools: None,
        tool_choice: None,
    };

    match llm_client.complete(req).await {
        Ok(resp) => {
            match session_runner
                .compact_history(session_id, resp.content, keep_recent)
                .await
            {
                Ok(()) => println!(
                    "[compaction] ✓ {} messages condensed, {} recent messages kept\n",
                    total - keep_recent * 2,
                    keep_recent * 2
                ),
                Err(e) => println!("[compaction] Failed to apply: {}\n", e),
            }
        }
        Err(e) => println!("[compaction] Summarization failed: {}\n", e),
    }
}

/// Perform self-improvement analysis — reflects on the session and proposes one improvement.
async fn do_self_improve(
    runner_arc: &Arc<Mutex<PipelineRunner>>,
    session_runner: &SessionRunner,
    session_id: &verdict::prelude::SessionId,
) {
    println!();
    println!("[improve] Analyzing session...");

    // Get session metadata for turn count context
    let turn_count = match session_runner.get_meta(session_id).await {
        Ok(m) => m.turn_count,
        Err(_) => 0,
    };

    if turn_count == 0 {
        println!("[improve] No conversation history yet — have a conversation first.");
        println!();
        return;
    }

    // Build single-step improve agent
    let improve_agent = Agent {
        name: "improve".into(),
        description: "Self-improvement analysis".into(),
        pipeline: build_improve_pipeline(),
        tools: ToolSet::None,
        skills: SkillSet { skills: vec![] },
        policy: AgentPolicy::default(),
        scorers: Vec::new(),
    };

    // Pass turn count as plain string so {input} resolves cleanly
    let input = serde_json::Value::String(format!("{}", turn_count));

    let result = {
        let mut runner = runner_arc.lock().await;
        runner
            .run(&improve_agent.pipeline, &improve_agent, input)
            .await
    };

    match result {
        Ok(pipeline_result) => {
            telemetry::export_telemetry(&pipeline_result.audit_log).await;

            // The single step is named "reflect_and_propose"
            let raw = pipeline_result
                .step_results
                .get("reflect_and_propose")
                .map(|r| r.output.raw.clone())
                .unwrap_or_default();

            if raw.is_empty() {
                println!("[improve] No proposal generated.\n");
                return;
            }

            // Try to parse as JSON for pretty display, fall back to raw text
            match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(json) => {
                    println!("[improve] ✦ Self-Improvement Proposal");
                    println!();
                    if let Some(finding) = json.get("finding").and_then(|v| v.as_str()) {
                        println!("  Finding:   {}", finding);
                    }
                    if let Some(proposal) = json.get("proposal").and_then(|v| v.as_str()) {
                        println!("  Proposal:  {}", proposal);
                    }
                    if let Some(risk) = json.get("risk_level").and_then(|v| v.as_str()) {
                        println!("  Risk:      {}", risk);
                    }
                }
                Err(_) => {
                    // Non-JSON response — show as-is
                    println!("[improve] Proposal:");
                    println!();
                    println!("{}", raw);
                }
            }

            println!();
            print!("Acknowledge this proposal? [y/N]: ");
            use std::io::Write;
            std::io::stdout().flush().ok();

            let ans = tokio::task::spawn_blocking(|| {
                let mut buf = String::new();
                std::io::stdin().read_line(&mut buf).ok();
                buf
            })
            .await
            .unwrap_or_default();

            if ans.trim().eq_ignore_ascii_case("y") {
                println!("[improve] ✓ Noted. Keep this in mind for future sessions.");
            } else {
                println!("[improve] Dismissed.");
            }
            println!();
        }
        Err(e) => {
            println!("[improve] Analysis failed: {}\n", e);
        }
    }
}

/// Echo mode when no LLM is configured
async fn run_echo_mode(agent_name: String) {
    let agent = build_echo_agent(&agent_name);
    let mut agent_registry = AgentRegistry::new();
    agent_registry.register(agent);

    let runner = PipelineRunner::with_agent_registry(Arc::new(agent_registry));
    let runner = Arc::new(Mutex::new(runner));
    let session_runner = SessionRunner::new(runner);

    let session_id = match session_runner
        .new_session(&agent_name, SessionPolicy::default())
        .await
    {
        Ok(id) => id,
        Err(e) => {
            error!(error = %e, "failed to create session");
            return;
        }
    };

    println!("verdict-app [echo mode] — /quit to exit");
    println!();

    loop {
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush().ok();

        let line = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            match std::io::stdin().read_line(&mut buf) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(buf),
            }
        })
        .await
        .unwrap_or(None);

        let line = match line {
            Some(l) => l,
            None => break,
        };

        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed == "/quit" || trimmed == "/exit" {
            break;
        }

        let turn = UserTurn {
            content: TurnContent { text: trimmed },
            attachments: vec![],
            interrupt_previous: false,
        };

        match session_runner.turn(&session_id, turn).await {
            Ok(TurnResult::Completed { output, .. }) => {
                println!();
                println!("assistant: {}", output);
                println!();
            }
            _ => {
                println!();
                println!("[echo] (no LLM response)");
                println!();
            }
        }
    }

    session_runner.close(&session_id).await.ok();
    println!();
    println!("Goodbye.");
}
