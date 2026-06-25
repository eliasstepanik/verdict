use axum::{
    extract::{Path, State},
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;

/// Verdict REST API server
pub struct VerdictServer {
    runner: Arc<verdict::runner::PipelineRunner>,
}

impl VerdictServer {
    /// Create a new Verdict server
    pub fn new(runner: Arc<verdict::runner::PipelineRunner>) -> Self {
        Self { runner }
    }

    /// Start the Verdict server on the given address
    pub async fn serve(self, addr: std::net::SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
        // App state for sharing resources across handlers
        #[derive(Clone)]
        struct AppState {
            runner: Arc<verdict::runner::PipelineRunner>,
        }

        let app_state = AppState {
            runner: self.runner.clone(),
        };

        // Health check endpoint
        async fn health_handler() -> impl IntoResponse {
            Json(json!({ "status": "ok" }))
        }

        // List all agents endpoint
        async fn agents_list_handler(State(state): State<AppState>) -> impl IntoResponse {
            let agents = state.runner.agent_registry.list();
            Json(json!({ "agents": agents }))
        }

        // Get agent details endpoint
        async fn agent_details_handler(
            Path(name): Path<String>,
            State(state): State<AppState>,
        ) -> impl IntoResponse {
            if let Some(agent) = state.runner.agent_registry.get(&name) {
                let step_names: Vec<String> = agent
                    .pipeline
                    .steps
                    .iter()
                    .map(|step| step.name.clone())
                    .collect();

                Json(json!({
                    "name": agent.name,
                    "description": agent.description,
                    "pipeline_steps": step_names,
                }))
            } else {
                Json(json!({
                    "error": format!("Agent '{}' not found", name),
                }))
            }
        }

        // Swagger UI endpoint (hardcoded HTML)
        async fn swagger_handler() -> impl IntoResponse {
            let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>Verdict REST API</title>
    <style>
        body { font-family: monospace; margin: 20px; background: #f5f5f5; }
        h1 { color: #333; }
        .endpoint { background: white; padding: 15px; margin: 10px 0; border-radius: 5px; border-left: 4px solid #0066cc; }
        .endpoint.get { border-left-color: #00cc00; }
        .endpoint.post { border-left-color: #cc6600; }
        code { background: #f0f0f0; padding: 2px 6px; border-radius: 3px; }
    </style>
</head>
<body>
    <h1>Verdict REST API</h1>
    
    <div class="endpoint get">
        <h3>GET /health</h3>
        <p>Check server health status</p>
        <code>{ "status": "ok" }</code>
    </div>

    <div class="endpoint get">
        <h3>GET /agents</h3>
        <p>List all registered agents</p>
        <code>{ "agents": ["coder", "reviewer", "debugger", ...] }</code>
    </div>

    <div class="endpoint get">
        <h3>GET /agents/{name}</h3>
        <p>Get details about a specific agent</p>
        <code>{ "name": "coder", "description": "...", "pipeline_steps": [...] }</code>
    </div>

    <div class="endpoint post">
        <h3>POST /agents/{name}/run</h3>
        <p>Run an agent with input</p>
        <p><strong>Request body:</strong></p>
        <code>{ "input": { /* your input */ } }</code>
        <p><strong>Response:</strong></p>
        <code>{ "success": true, "result": {...} }</code>
    </div>
</body>
</html>
            "#;

            axum::response::Html(html)
        }

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/swagger-ui", get(swagger_handler))
            .route("/agents", get(agents_list_handler))
            .route("/agents/:name", get(agent_details_handler))
            .with_state(app_state);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verdict_server_creation() {
        let runner = Arc::new(verdict::runner::PipelineRunner::new());
        let _server = VerdictServer::new(runner);
    }
}
