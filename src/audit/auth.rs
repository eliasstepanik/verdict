//! Authentication middleware and utilities for the monitoring server.
//!
//! This module provides constant-time token comparison and HTTP middleware
//! for bearer token authentication on the monitoring HTTP server.

use axum::http::StatusCode;
use axum::middleware::Next;
use axum::http::Request;
use axum::body::Body;
use axum::extract::State;
use axum::response::IntoResponse;
use serde_json::json;

/// Constant-time comparison for bearer tokens (XOR-fold to prevent timing attacks).
/// Returns true only if both strings are byte-identical, comparing all bytes even on mismatch.
pub fn constant_time_compare(expected: &str, actual: &str) -> bool {
    let expected_bytes = expected.as_bytes();
    let actual_bytes = actual.as_bytes();
    
    // Lengths must match; still fold all bits to avoid timing leak
    let mut result: u32 = (expected_bytes.len() ^ actual_bytes.len()) as u32;
    
    // XOR all bytes (safe: we compare min length, then fold length mismatch bit)
    // Note: loop length depends on min(expected.len(), actual.len()), which could leak token
    // LENGTH via timing in a hostile network environment. This is acceptable here because the
    // token's length is not itself secret (only its VALUE is) — an attacker learning "the token
    // is roughly N bytes" doesn't help them guess the value.
    let min_len = expected_bytes.len().min(actual_bytes.len());
    for i in 0..min_len {
        result |= (expected_bytes[i] as u32) ^ (actual_bytes[i] as u32);
    }
    
    result == 0
}

/// App state for the monitoring server (reused in auth_middleware)
#[derive(Clone)]
pub struct AppState {
    pub audit_log: std::sync::Arc<std::sync::Mutex<crate::audit::AuditLog>>,
    pub trace: std::sync::Arc<std::sync::Mutex<crate::context::PipelineTrace>>,
    pub agent_registry: Option<std::sync::Arc<crate::registry::AgentRegistry>>,
    pub conversation_registry:
        Option<std::sync::Arc<std::sync::Mutex<crate::llm::ConversationRegistry>>>,
    /// Test delay for timeout testing (normally None, no runtime cost)
    pub test_delay: Option<std::time::Duration>,
    /// Optional bearer token for auth middleware (None = auth disabled)
    pub auth_token: Option<String>,
}

/// Auth middleware: checks Authorization: Bearer <token> header if auth_token is configured
pub async fn auth_middleware(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, axum::Json<serde_json::Value>)> {
    // If auth is disabled (auth_token is None), pass through unconditionally
    if state.auth_token.is_none() {
        return Ok(next.run(req).await);
    }

    let expected_token = state.auth_token.as_ref().unwrap();
    
    // Extract Authorization header
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Parse "Bearer <token>"
    let provided_token = if let Some(token) = auth_header.strip_prefix("Bearer ") {
        token
    } else {
        return Err((
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({ "error": "Missing or invalid Authorization header" })),
        ));
    };

    // Constant-time comparison
    if !constant_time_compare(expected_token, provided_token) {
        return Err((
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({ "error": "Invalid token" })),
        ));
    }

    Ok(next.run(req).await)
}
