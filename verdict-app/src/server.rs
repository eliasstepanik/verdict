//! Server mode: run AgentServer with StdioTransport

use crate::agent::{build_assistant_agent, build_echo_agent};
use crate::config::AppConfig;
use crate::memory;
use std::sync::Arc;
use tokio::sync::Mutex;
use verdict::prelude::*;

pub async fn run(config: AppConfig) {
    // Build agent based on LLM availability
    let agent = if config.build_llm_client().is_some() {
        build_assistant_agent(&config, "assistant")
    } else {
        build_echo_agent("assistant")
    };

    // Setup registries and runner
    let mut agent_registry = AgentRegistry::new();
    agent_registry.register(agent);

    let mut runner = PipelineRunner::with_agent_registry(Arc::new(agent_registry));

    // Wire in memory store
    runner = runner.with_memory(memory::build_memory_store());

    // Add LLM client if available
    if let Some(llm) = config.build_llm_client() {
        runner = runner.with_llm_client(Arc::new(llm));
    }

    // Note: could also add output_sink for streaming with:
    // runner = runner.with_output_sink(Arc::new(sink))
    //
    // To wire in VerdictServer (Phase 12), use:
    // let monitoring_server = MonitoringServer::new(
    //     Arc::new(Mutex::new(runner.audit_log.clone())),
    //     Arc::new(Mutex::new(PipelineTrace::new())),
    // );
    // tokio::spawn(async move { monitoring_server.serve("127.0.0.1:8080".parse().unwrap()).await });

    let runner = Arc::new(Mutex::new(runner));
    let session_runner = Arc::new(SessionRunner::new(runner));

    let transport = Arc::new(StdioTransport::new());
    let server = AgentServer::new(session_runner, transport);

    if let Err(e) = server.run().await {
        eprintln!("Server error: {}", e);
        std::process::exit(1);
    }
}
