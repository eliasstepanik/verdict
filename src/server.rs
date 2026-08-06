//! Server/Daemon Mode - JSON-RPC transport for remote agent execution
//!
//! Verdict can run as a daemon that accepts work over IPC, initially via stdio JSON-RPC.
//! This module provides the server infrastructure for accepting client requests and sending responses.

use crate::session::{SessionId, SessionPolicy, SessionRunner, TurnContent, TurnResult, UserTurn};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A server that accepts work over a transport (initially stdio JSON-RPC)
pub struct AgentServer {
    session_runner: Arc<SessionRunner>,
    transport: Arc<dyn ServerTransport>,
    policy: ServerPolicy,
}

/// Policy governing the server
#[derive(Debug, Clone)]
pub struct ServerPolicy {
    pub max_concurrent_sessions: usize,
    pub allowed_agents: Vec<String>,
    pub require_auth_token: Option<String>,
}

impl Default for ServerPolicy {
    fn default() -> Self {
        ServerPolicy {
            max_concurrent_sessions: 10,
            allowed_agents: vec![],
            require_auth_token: None,
        }
    }
}

/// Transport trait — receive requests, send events
#[async_trait]
pub trait ServerTransport: Send + Sync {
    async fn next_request(&self) -> Result<Option<ClientRequest>, ServerError>;
    async fn send_event(&self, event: ServerEvent) -> Result<(), ServerError>;
    async fn shutdown(&self) -> Result<(), ServerError>;
}

/// Requests from client to server
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientRequest {
    NewSession {
        id: String,
        agent: String,
        policy: Option<serde_json::Value>,
    },
    Turn {
        session_id: String,
        content: String,
        attachments: Vec<String>,
    },
    CancelTurn {
        session_id: String,
    },
    CloseSession {
        session_id: String,
    },
    ListSessions,
    Ping,
}

/// Events from server to client
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    SessionCreated {
        session_id: String,
    },
    Chunk {
        session_id: String,
        delta: String,
    },
    TurnCompleted {
        session_id: String,
        output: String,
        success: bool,
    },
    SessionClosed {
        session_id: String,
    },
    SessionList {
        sessions: Vec<String>,
    },
    Error {
        session_id: Option<String>,
        message: String,
    },
    Pong,
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Serialization error: {0}")]
    Serialization(String),
    #[error("Session error: {0}")]
    Session(String),
    #[error("Policy violation: {0}")]
    PolicyViolation(String),
    #[error("Transport closed")]
    TransportClosed,
}

impl AgentServer {
    pub fn new(session_runner: Arc<SessionRunner>, transport: Arc<dyn ServerTransport>) -> Self {
        AgentServer {
            session_runner,
            transport,
            policy: ServerPolicy::default(),
        }
    }

    pub fn with_policy(mut self, policy: ServerPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Run the server loop until transport closes or error
    pub async fn run(&self) -> Result<(), ServerError> {
        loop {
            match self.transport.next_request().await? {
                None => break,
                Some(req) => {
                    let event = self.handle_request(req).await;
                    self.transport.send_event(event).await?;
                }
            }
        }
        self.transport.shutdown().await?;
        Ok(())
    }

    async fn handle_request(&self, req: ClientRequest) -> ServerEvent {
        match req {
            ClientRequest::Ping => ServerEvent::Pong,
            ClientRequest::ListSessions => {
                let sessions = self.session_runner.list().await;
                ServerEvent::SessionList {
                    sessions: sessions.iter().map(|m| m.id.to_string()).collect(),
                }
            }
            ClientRequest::NewSession {
                id,
                agent,
                policy: _,
            } => {
                // Check policy
                if !self.policy.allowed_agents.is_empty()
                    && !self.policy.allowed_agents.contains(&agent)
                {
                    return ServerEvent::Error {
                        session_id: Some(id.clone()),
                        message: format!("Agent '{}' not in allowed list", agent),
                    };
                }
                let sess_policy = SessionPolicy::default();
                match self.session_runner.new_session(&agent, sess_policy).await {
                    Ok(sess_id) => ServerEvent::SessionCreated {
                        session_id: sess_id.to_string(),
                    },
                    Err(e) => ServerEvent::Error {
                        session_id: Some(id),
                        message: e.to_string(),
                    },
                }
            }
            ClientRequest::Turn {
                session_id,
                content,
                attachments: _,
            } => {
                let sess_id = SessionId::from(session_id.clone());
                let turn = UserTurn {
                    content: TurnContent { text: content },
                    attachments: vec![],
                    interrupt_previous: false,
                };
                match self.session_runner.turn(&sess_id, turn).await {
                    Ok(TurnResult::Completed { output, .. }) => ServerEvent::TurnCompleted {
                        session_id,
                        output,
                        success: true,
                    },
                    Ok(TurnResult::Cancelled { partial_output, .. }) => {
                        ServerEvent::TurnCompleted {
                            session_id,
                            output: partial_output,
                            success: false,
                        }
                    }
                    Ok(other) => ServerEvent::TurnCompleted {
                        session_id,
                        output: format!("{:?}", other),
                        success: false,
                    },
                    Err(e) => ServerEvent::Error {
                        session_id: Some(session_id),
                        message: e.to_string(),
                    },
                }
            }
            ClientRequest::CancelTurn { session_id } => {
                let sess_id = SessionId::from(session_id.clone());
                match self.session_runner.cancel_turn(&sess_id).await {
                    Ok(()) => ServerEvent::TurnCompleted {
                        session_id,
                        output: String::new(),
                        success: false,
                    },
                    Err(e) => ServerEvent::Error {
                        session_id: Some(session_id),
                        message: e.to_string(),
                    },
                }
            }
            ClientRequest::CloseSession { session_id } => {
                let sess_id = SessionId::from(session_id.clone());
                match self.session_runner.close(&sess_id).await {
                    Ok(_) => ServerEvent::SessionClosed { session_id },
                    Err(e) => ServerEvent::Error {
                        session_id: Some(session_id),
                        message: e.to_string(),
                    },
                }
            }
        }
    }
}

/// Stdio JSON-RPC transport (newline-delimited JSON)
///
/// For simplicity, uses blocking I/O wrapped in tokio::task::block_in_place
/// to avoid creating/managing Mutex-wrapped stdio handles
pub struct StdioTransport;

impl StdioTransport {
    pub fn new() -> Self {
        StdioTransport
    }
}

#[async_trait]
impl ServerTransport for StdioTransport {
    async fn next_request(&self) -> Result<Option<ClientRequest>, ServerError> {
        use std::io::BufRead;

        let line = tokio::task::block_in_place(|| {
            let mut line = String::new();
            let stdin = std::io::stdin();
            let mut handle = stdin.lock();
            let n = handle
                .read_line(&mut line)
                .map_err(|e| ServerError::Io(e.to_string()))?;
            if n == 0 {
                Ok::<Option<String>, ServerError>(None)
            } else {
                Ok(Some(line))
            }
        })?;

        match line {
            None => Ok(None),
            Some(line) => {
                let req: ClientRequest = serde_json::from_str(line.trim())
                    .map_err(|e| ServerError::Serialization(e.to_string()))?;
                Ok(Some(req))
            }
        }
    }

    async fn send_event(&self, event: ServerEvent) -> Result<(), ServerError> {
        use std::io::Write;

        let json =
            serde_json::to_string(&event).map_err(|e| ServerError::Serialization(e.to_string()))?;

        tokio::task::block_in_place(|| {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle
                .write_all(json.as_bytes())
                .map_err(|e| ServerError::Io(e.to_string()))?;
            handle
                .write_all(b"\n")
                .map_err(|e| ServerError::Io(e.to_string()))?;
            handle.flush().map_err(|e| ServerError::Io(e.to_string()))?;
            Ok::<(), ServerError>(())
        })
    }

    async fn shutdown(&self) -> Result<(), ServerError> {
        Ok(())
    }
}
