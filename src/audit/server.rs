use crate::audit::AuditLog;
use serde_json::json;
use tracing::error;

/// Monitoring server for Web UI dashboard (Phase E)
pub struct MonitoringServer {
    audit_log: std::sync::Arc<std::sync::Mutex<AuditLog>>,
    trace: std::sync::Arc<std::sync::Mutex<crate::context::PipelineTrace>>,
    agent_registry: Option<std::sync::Arc<crate::registry::AgentRegistry>>,
    conversation_registry:
        Option<std::sync::Arc<std::sync::Mutex<crate::llm::ConversationRegistry>>>,
    timeout_duration: std::time::Duration,
    /// Delay for testing timeout behavior (only set via test constructors, never from client input)
    test_delay: Option<std::time::Duration>,
}

impl MonitoringServer {
    /// Create a new monitoring server with default 30s timeout
    pub fn new(audit_log: AuditLog, trace: crate::context::PipelineTrace) -> Self {
        Self {
            audit_log: std::sync::Arc::new(std::sync::Mutex::new(audit_log)),
            trace: std::sync::Arc::new(std::sync::Mutex::new(trace)),
            agent_registry: None,
            conversation_registry: None,
            timeout_duration: std::time::Duration::from_secs(30),
            test_delay: None,
        }
    }

    /// Create a new monitoring server with custom timeout duration
    pub fn with_timeout(
        audit_log: AuditLog,
        trace: crate::context::PipelineTrace,
        timeout: std::time::Duration,
    ) -> Self {
        Self {
            audit_log: std::sync::Arc::new(std::sync::Mutex::new(audit_log)),
            trace: std::sync::Arc::new(std::sync::Mutex::new(trace)),
            agent_registry: None,
            conversation_registry: None,
            timeout_duration: timeout,
            test_delay: None,
        }
    }

    /// Add agent registry to monitoring server
    pub fn with_agent_registry(
        mut self,
        registry: std::sync::Arc<crate::registry::AgentRegistry>,
    ) -> Self {
        self.agent_registry = Some(registry);
        self
    }

    /// Add conversation registry to monitoring server
    pub fn with_conversation_registry(
        mut self,
        registry: std::sync::Arc<std::sync::Mutex<crate::llm::ConversationRegistry>>,
    ) -> Self {
        self.conversation_registry = Some(registry);
        self
    }

    /// Test-only constructor helper: create a server with delay injection for timeout testing
    /// Used only by tests to simulate slow handlers for timeout validation.
    /// In production code, test_delay remains None and has no performance impact.
    pub fn with_test_delay(mut self, delay: std::time::Duration) -> Self {
        self.test_delay = Some(delay);
        self
    }

    /// Start the monitoring HTTP server on the given address
    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        use axum::{
            extract::State,
            response::{Html as AxumHtml, IntoResponse},
            routing::get,
            Json, Router,
        };

        let audit_log = self.audit_log.clone();
        let trace = self.trace.clone();
        let agent_registry = self.agent_registry.clone();
        let conversation_registry = self.conversation_registry.clone();
        let test_delay = self.test_delay;

        // App state structure
        #[derive(Clone)]
        struct AppState {
            audit_log: std::sync::Arc<std::sync::Mutex<AuditLog>>,
            trace: std::sync::Arc<std::sync::Mutex<crate::context::PipelineTrace>>,
            agent_registry: Option<std::sync::Arc<crate::registry::AgentRegistry>>,
            conversation_registry:
                Option<std::sync::Arc<std::sync::Mutex<crate::llm::ConversationRegistry>>>,
            /// Test delay for timeout testing (normally None, no runtime cost)
            test_delay: Option<std::time::Duration>,
        }

        let app_state = AppState {
            audit_log: audit_log.clone(),
            trace: trace.clone(),
            agent_registry: agent_registry.clone(),
            conversation_registry: conversation_registry.clone(),
            test_delay,
        };

        // Handlers
        async fn index_handler() -> impl IntoResponse {
            let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Verdict Monitoring Dashboard</title>
    <style>
        body { font-family: monospace; margin: 20px; background: #f5f5f5; }
        h1 { color: #333; }
        .section { background: white; padding: 20px; margin: 10px 0; border-radius: 5px; }
        .entry { padding: 10px; border-left: 3px solid #0066cc; margin: 5px 0; }
        .error { border-left-color: #cc0000; }
        .success { border-left-color: #00cc00; }
        table { width: 100%; border-collapse: collapse; }
        th, td { padding: 8px; text-align: left; border-bottom: 1px solid #ddd; }
        th { background-color: #f2f2f2; }
    </style>
</head>
<body>
    <h1>Verdict Monitoring Dashboard</h1>
    <div class="section">
        <h2>Registered Agents</h2>
        <p><a href="/api/agents">View all agents (JSON)</a></p>
    </div>
    <div class="section">
        <h2>Conversations</h2>
        <p><a href="/api/conversations">View conversations (JSON)</a></p>
    </div>
    <div class="section">
        <h2>Recent Audit Entries</h2>
        <p><a href="/api/entries">View all entries (JSON)</a></p>
    </div>
    <div class="section">
        <h2>Pipeline Trace</h2>
        <p><a href="/api/trace">View trace (JSON)</a></p>
    </div>
</body>
</html>
            "#;
            AxumHtml(html)
        }

        async fn entries_handler(
            State(state): State<AppState>,
        ) -> Json<Vec<crate::audit::AuditEntry>> {
            // Test-only delay injection via AppState (safe, server-side only, not client-controlled)
            if let Some(delay) = state.test_delay {
                tokio::time::sleep(delay).await;
            }
            
            let log = state.audit_log.lock().ok();
            let entries: Vec<_> = log
                .map(|l| {
                    let mut all_entries = l.entries();
                    all_entries.reverse();
                    all_entries.into_iter().take(100).collect()
                })
                .unwrap_or_default();
            Json(entries)
        }

        async fn trace_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
            match state.trace.lock() {
                Ok(t) => {
                    let entries = t.entries.clone();
                    Json(json!({ "entries": entries }))
                }
                Err(e) => {
                    error!(error = %e, "trace mutex poisoned");
                    Json(json!({ "error": "trace mutex poisoned", "entries": [] }))
                }
            }
        }

        async fn agents_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
            if let Some(registry) = &state.agent_registry {
                let agents = registry.list_agents();
                let agent_list: Vec<serde_json::Value> = agents
                    .iter()
                    .map(|agent| {
                        json!({
                            "name": agent.name,
                            "description": agent.description,
                        })
                    })
                    .collect();
                Json(json!({ "agents": agent_list }))
            } else {
                Json(json!({ "agents": [], "error": "agent registry not available" }))
            }
        }

        async fn conversations_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
            if let Some(registry) = &state.conversation_registry {
                match registry.lock() {
                    Ok(r) => {
                        let conversations = r.list_conversations();
                        let conv_list: Vec<serde_json::Value> = conversations
                            .iter()
                            .map(|(id, history)| {
                                json!({
                                    "id": id,
                                    "message_count": history.messages.len(),
                                })
                            })
                            .collect();
                        Json(json!({ "conversations": conv_list }))
                    }
                    Err(_) => {
                        Json(json!({ "conversations": [], "error": "registry mutex poisoned" }))
                    }
                }
            } else {
                Json(json!({ "conversations": [], "error": "conversation registry not available" }))
            }
        }

        let app = Router::new()
            .route("/", get(index_handler))
            .route("/api/entries", get(entries_handler))
            .route("/api/trace", get(trace_handler))
            .route("/api/agents", get(agents_handler))
            .route("/api/conversations", get(conversations_handler))
            .with_state(app_state)
            .layer(tower_http::timeout::TimeoutLayer::new(self.timeout_duration));

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}
