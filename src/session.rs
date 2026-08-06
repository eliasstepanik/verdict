//! Interactive Sessions — long-lived conversations with persistent state

use std::sync::Arc;
use std::path::PathBuf;
use std::collections::HashMap;
use tokio::sync::Mutex;
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use serde_json::Value;

use crate::runner::PipelineRunner;
use crate::llm::MessageHistory;

/// Unique identifier for a session
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    /// Generate a new random session ID
    pub fn new() -> Self {
        SessionId(Uuid::new_v4().to_string())
    }

    /// Get the session ID as a string reference
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for SessionId {
    fn from(s: String) -> Self {
        SessionId(s)
    }
}

impl From<&str> for SessionId {
    fn from(s: &str) -> Self {
        SessionId(s.to_string())
    }
}

/// Content of a user's turn (text for now; multi-modal later)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnContent {
    pub text: String,
}

/// An attachment passed with a user turn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub content: String,
    pub mime_type: Option<String>,
}

/// A single turn submitted by the user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserTurn {
    pub content: TurnContent,
    pub attachments: Vec<Attachment>,
    pub interrupt_previous: bool,
}

impl UserTurn {
    /// Create a turn from text only
    pub fn text(text: impl Into<String>) -> Self {
        UserTurn {
            content: TurnContent {
                text: text.into(),
            },
            attachments: vec![],
            interrupt_previous: false,
        }
    }
}

/// Token usage for a single turn
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// The result of executing a single user turn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TurnResult {
    /// Turn completed successfully
    Completed { output: String, usage: TokenUsage },

    /// Turn was cancelled by user
    Cancelled {
        partial_output: String,
        last_completed_step: Option<String>,
        resumable: bool,
    },

    /// A guard failed
    GuardFailed { guard: String, reason: String },

    /// Awaiting user input
    AwaitingInput { prompt: String },

    /// Error during turn
    Error(String),
}

/// Policy governing a session's resource limits and lifecycle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionPolicy {
    pub max_turns: Option<u32>,
    pub max_total_tokens: Option<u32>,
    pub max_cost_usd: Option<f64>,
    /// Seconds of idle time before session is considered timed out
    pub idle_timeout_seconds: Option<u64>,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        SessionPolicy {
            max_turns: Some(1000),
            max_total_tokens: None,
            max_cost_usd: None,
            idle_timeout_seconds: Some(3600), // 1 hour
        }
    }
}

/// A single message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: ConversationRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub turn_index: u32,
    pub usage: Option<TokenUsage>,
}

/// Role of a conversation participant
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConversationRole {
    User,
    Assistant,
    System,
}

/// Persistent conversation history for a session
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationHistory {
    pub messages: Vec<ConversationMessage>,
    pub turn_count: u32,
    pub total_tokens: TokenUsage,
}

impl ConversationHistory {
    /// Create a new empty conversation history
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a user message to the conversation
    pub fn push_user(&mut self, content: String, turn_index: u32) {
        self.messages.push(ConversationMessage {
            role: ConversationRole::User,
            content,
            timestamp: Utc::now(),
            turn_index,
            usage: None,
        });
    }

    /// Add an assistant message to the conversation
    pub fn push_assistant(
        &mut self,
        content: String,
        turn_index: u32,
        usage: Option<TokenUsage>,
    ) {
        self.messages.push(ConversationMessage {
            role: ConversationRole::Assistant,
            content,
            timestamp: Utc::now(),
            turn_index,
            usage: usage.clone(),
        });
        if let Some(u) = usage {
            self.total_tokens.prompt_tokens += u.prompt_tokens;
            self.total_tokens.completion_tokens += u.completion_tokens;
            self.total_tokens.total_tokens += u.total_tokens;
        }
        self.turn_count += 1;
    }

    /// Get the last N messages
    pub fn last_n_messages(&self, n: usize) -> Vec<&ConversationMessage> {
        let skip = self.messages.len().saturating_sub(n);
        self.messages[skip..].iter().collect()
    }

    /// Convert to MessageHistory for LLM requests
    pub fn to_message_history(&self) -> MessageHistory {
        let mut history = MessageHistory::new();
        for msg in &self.messages {
            let role = match msg.role {
                ConversationRole::User => crate::llm::ChatRole::User,
                ConversationRole::Assistant => crate::llm::ChatRole::Assistant,
                ConversationRole::System => crate::llm::ChatRole::System,
            };
            history.push(role, msg.content.clone());
        }
        history
    }
}

/// Metadata about a session (for listing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: SessionId,
    pub agent_name: String,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub turn_count: u32,
    pub total_tokens: TokenUsage,
}

/// The status of a session
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    Active,
    Idle,
    Closed,
    TimedOut,
}

/// A long-lived session that accepts user turns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub agent_name: String,
    pub history: ConversationHistory,
    pub policy: SessionPolicy,
    pub status: SessionStatus,
    pub created_at: DateTime<Utc>,
    pub last_active_at: DateTime<Utc>,
    pub scratchpad: HashMap<String, Value>,
}

impl Session {
    /// Create a new session for the given agent
    pub fn new(agent_name: impl Into<String>, policy: SessionPolicy) -> Self {
        let now = Utc::now();
        Session {
            id: SessionId::new(),
            agent_name: agent_name.into(),
            history: ConversationHistory::new(),
            policy,
            status: SessionStatus::Idle,
            created_at: now,
            last_active_at: now,
            scratchpad: HashMap::new(),
        }
    }

    /// Get session metadata
    pub fn meta(&self) -> SessionMeta {
        SessionMeta {
            id: self.id.clone(),
            agent_name: self.agent_name.clone(),
            created_at: self.created_at,
            last_active_at: self.last_active_at,
            turn_count: self.history.turn_count,
            total_tokens: self.history.total_tokens.clone(),
        }
    }

    /// Check if turn limit has been exceeded
    pub fn is_turn_limit_exceeded(&self) -> bool {
        if let Some(max) = self.policy.max_turns {
            self.history.turn_count >= max
        } else {
            false
        }
    }

    /// Check if token limit has been exceeded
    pub fn is_token_limit_exceeded(&self) -> bool {
        if let Some(max) = self.policy.max_total_tokens {
            self.history.total_tokens.total_tokens >= max
        } else {
            false
        }
    }
}

/// Errors from SessionRunner operations
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("Session not found: {0}")]
    NotFound(String),

    #[error("Session limit exceeded: {0}")]
    LimitExceeded(String),

    #[error("Session is closed")]
    Closed,

    #[error("Agent not found: {0}")]
    AgentNotFound(String),

    #[error("Persistence error: {0}")]
    PersistenceError(String),

    #[error("Pipeline error: {0}")]
    PipelineError(String),
}

/// Manages multiple long-lived sessions
pub struct SessionRunner {
    sessions: Arc<Mutex<HashMap<SessionId, Session>>>,
    runner: Arc<Mutex<PipelineRunner>>,
    persist_dir: Option<PathBuf>,
}

impl SessionRunner {
    /// Create a new SessionRunner
    pub fn new(runner: Arc<Mutex<PipelineRunner>>) -> Self {
        SessionRunner {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            runner,
            persist_dir: None,
        }
    }

    /// Enable persistence by setting a directory
    pub fn with_persistence(mut self, dir: PathBuf) -> Self {
        self.persist_dir = Some(dir);
        self
    }

    /// Create a new session for the named agent
    pub async fn new_session(
        &self,
        agent_name: &str,
        policy: SessionPolicy,
    ) -> Result<SessionId, SessionError> {
        // Verify agent exists
        let runner = self.runner.lock().await;
        let agent = runner.agent_registry
            .get(agent_name)
            .ok_or_else(|| SessionError::AgentNotFound(agent_name.to_string()))?;
        let _ = agent; // just validating existence
        drop(runner);

        let session = Session::new(agent_name, policy);
        let id = session.id.clone();

        let mut sessions = self.sessions.lock().await;
        sessions.insert(id.clone(), session.clone());
        drop(sessions);

        // Persist immediately
        if let Some(dir) = &self.persist_dir {
            self.save_session_to_disk(dir, &session).await?;
        }

        Ok(id)
    }

    /// Resume a session by ID (loads from disk if not in memory)
    pub async fn resume(&self, id: &SessionId) -> Result<SessionMeta, SessionError> {
        {
            let sessions = self.sessions.lock().await;
            if let Some(s) = sessions.get(id) {
                return Ok(s.meta());
            }
        }

        // Try loading from disk
        if let Some(dir) = &self.persist_dir {
            let session = self.load_session_from_disk(dir, id).await?;
            let meta = session.meta();
            let mut sessions = self.sessions.lock().await;
            sessions.insert(id.clone(), session);
            return Ok(meta);
        }

        Err(SessionError::NotFound(id.to_string()))
    }

    /// Execute one user turn in the session
    pub async fn turn(
        &self,
        id: &SessionId,
        input: UserTurn,
    ) -> Result<TurnResult, SessionError> {
        // Get agent name and validate limits
        let (agent_name, turn_index) = {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .get_mut(id)
                .ok_or_else(|| SessionError::NotFound(id.to_string()))?;

            if session.status == SessionStatus::Closed {
                return Err(SessionError::Closed);
            }
            if session.is_turn_limit_exceeded() {
                return Err(SessionError::LimitExceeded("max_turns exceeded".into()));
            }
            if session.is_token_limit_exceeded() {
                return Err(SessionError::LimitExceeded("max_total_tokens exceeded".into()));
            }

            session.status = SessionStatus::Active;
            session.last_active_at = Utc::now();

            let turn_idx = session.history.turn_count;
            (session.agent_name.clone(), turn_idx)
        };

        // Get agent
        let agent = {
            let runner = self.runner.lock().await;
            runner.agent_registry
                .get(&agent_name)
                .ok_or_else(|| SessionError::AgentNotFound(agent_name.clone()))?
        };


        // Pass user text directly as a string so {input} in pipeline templates
        // resolves to the user's actual message — not a confusing JSON blob.
        let pipeline_input = serde_json::Value::String(input.content.text.clone());

        // Record user message in history
        {
            let mut sessions = self.sessions.lock().await;
            let session = sessions.get_mut(id).unwrap();
            session.history.push_user(input.content.text.clone(), turn_index);
        }

        // Run the pipeline
        let pipeline = agent.pipeline.clone();
        let agent_ref = (*agent).clone();

        let result = {
            // Lock the runner and execute the pipeline
            let mut runner = self.runner.lock().await;
            runner.run(&pipeline, &agent_ref, pipeline_input).await
        };

        match result {
            Ok(pipeline_result) => {
                // Extract output from last passing step
                let output = pipeline_result
                    .step_results
                    .values()
                    .filter(|r| r.verdict_passed)
                    .last()
                    .map(|r| r.output.raw.clone())
                    .unwrap_or_default();

                // Estimate token usage from step results
                let usage = TokenUsage {
                    prompt_tokens: 0,
                    completion_tokens: (output.len() as u32) / 4, // rough estimate
                    total_tokens: (output.len() as u32) / 4,
                };

                // Record assistant response
                {
                    let mut sessions = self.sessions.lock().await;
                    if let Some(session) = sessions.get_mut(id) {
                        session
                            .history
                            .push_assistant(output.clone(), turn_index, Some(usage.clone()));
                        session.status = SessionStatus::Idle;

                        // Persist
                        if let Some(dir) = &self.persist_dir {
                            let session_clone = session.clone();
                            drop(sessions);
                            self.save_session_to_disk(dir, &session_clone).await?;
                        }
                    }
                }

                Ok(TurnResult::Completed { output, usage })
            }
            Err(e) => {
                // Mark session as idle on error
                {
                    let mut sessions = self.sessions.lock().await;
                    if let Some(session) = sessions.get_mut(id) {
                        session.status = SessionStatus::Idle;
                    }
                }
                Ok(TurnResult::Error(e.to_string()))
            }
        }
    }

    /// Close a session
    pub async fn close(&self, id: &SessionId) -> Result<SessionMeta, SessionError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| SessionError::NotFound(id.to_string()))?;
        session.status = SessionStatus::Closed;
        let meta = session.meta();
        Ok(meta)
    }

    /// List all active sessions
    pub async fn list(&self) -> Vec<SessionMeta> {
        let sessions = self.sessions.lock().await;
        sessions.values().map(|s| s.meta()).collect()
    }

    /// Get session metadata by ID
    pub async fn get_meta(&self, id: &SessionId) -> Result<SessionMeta, SessionError> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(id)
            .map(|s| s.meta())
            .ok_or_else(|| SessionError::NotFound(id.to_string()))
    }

    /// Cancel the current turn in a session
    pub async fn cancel_turn(&self, id: &SessionId) -> Result<(), SessionError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| SessionError::NotFound(id.to_string()))?;

        if session.status == SessionStatus::Closed {
            return Err(SessionError::Closed);
        }

        session.status = SessionStatus::Idle;
        Ok(())
    }

    // --- Persistence helpers ---

    async fn save_session_to_disk(
        &self,
        dir: &PathBuf,
        session: &Session,
    ) -> Result<(), SessionError> {
        tokio::fs::create_dir_all(dir).await.map_err(|e| {
            SessionError::PersistenceError(e.to_string())
        })?;
        let path = dir.join(format!("{}.json", session.id.as_str()));
        let json = serde_json::to_string_pretty(session).map_err(|e| {
            SessionError::PersistenceError(e.to_string())
        })?;
        tokio::fs::write(&path, json).await.map_err(|e| {
            SessionError::PersistenceError(e.to_string())
        })?;
        Ok(())
    }

    async fn load_session_from_disk(
        &self,
        dir: &PathBuf,
        id: &SessionId,
    ) -> Result<Session, SessionError> {
        let path = dir.join(format!("{}.json", id.as_str()));
        let json = tokio::fs::read_to_string(&path)
            .await
            .map_err(|_| SessionError::NotFound(id.to_string()))?;
        let session: Session = serde_json::from_str(&json).map_err(|e| {
            SessionError::PersistenceError(e.to_string())
        })?;
        Ok(session)
    }

    /// Get the full conversation history for a session
    pub async fn get_history(&self, id: &SessionId) -> Result<ConversationHistory, SessionError> {
        let sessions = self.sessions.lock().await;
        sessions
            .get(id)
            .map(|s| s.history.clone())
            .ok_or_else(|| SessionError::NotFound(id.to_string()))
    }

    /// Compact conversation history by replacing old messages with an LLM-generated summary.
    /// Keeps the `keep_recent_pairs` most recent user+assistant pairs verbatim.
    pub async fn compact_history(
        &self,
        id: &SessionId,
        summary: String,
        keep_recent_pairs: usize,
    ) -> Result<(), SessionError> {
        let mut sessions = self.sessions.lock().await;
        let session = sessions
            .get_mut(id)
            .ok_or_else(|| SessionError::NotFound(id.to_string()))?;

        let history = &mut session.history;
        let total = history.messages.len();
        let keep = (keep_recent_pairs * 2).min(total);

        // Extract the messages we want to keep (most recent)
        let recent: Vec<ConversationMessage> = if keep > 0 {
            history.messages[total - keep..].to_vec()
        } else {
            vec![]
        };

        // Build a synthetic summary message as System role
        let summary_msg = ConversationMessage {
            role: ConversationRole::System,
            content: format!("[Conversation summary — {} messages condensed]\n{}", total - keep, summary),
            timestamp: chrono::Utc::now(),
            turn_index: 0,
            usage: None,
        };

        // Replace history with summary + recent messages; reset token counts
        history.messages = std::iter::once(summary_msg).chain(recent).collect();
        history.total_tokens = TokenUsage::default();

        Ok(())
    }

    /// Record a completed user/assistant exchange into the session's history
    /// without running a pipeline turn. Used by callers (like DiscordBot's
    /// orchestrator) that execute the LLM call themselves and only need the
    /// session's persisted history updated afterward.
    pub async fn record_exchange(
        &self,
        id: &SessionId,
        user: String,
        assistant: String,
    ) -> Result<(), SessionError> {
        {
            let mut sessions = self.sessions.lock().await;
            let session = sessions
                .get_mut(id)
                .ok_or_else(|| SessionError::NotFound(id.to_string()))?;

            if session.status == SessionStatus::Closed {
                return Err(SessionError::Closed);
            }

            let turn_idx = session.history.turn_count;
            session.history.push_user(user, turn_idx);
            session.history.push_assistant(assistant, turn_idx, None);
            session.last_active_at = Utc::now();
        };

        // Persist to disk if enabled
        if let Some(dir) = &self.persist_dir {
            let sessions = self.sessions.lock().await;
            if let Some(session) = sessions.get(id) {
                let session_clone = session.clone();
                drop(sessions);
                self.save_session_to_disk(dir, &session_clone).await?;
            }
        }

        Ok(())
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::PipelineRunner;

    #[tokio::test]
    async fn test_record_exchange_success() {
        // Create a minimal PipelineRunner (empty agent registry)
        let runner = Arc::new(Mutex::new(PipelineRunner::new()));
        let session_runner = SessionRunner::new(runner);

        // Manually create a session for testing
        let session = Session::new("test_agent", SessionPolicy::default());
        let test_id = session.id.clone();
        {
            let mut sessions = session_runner.sessions.lock().await;
            sessions.insert(test_id.clone(), session);
        }

        // Call record_exchange
        let result = session_runner
            .record_exchange(
                &test_id,
                "hello world".to_string(),
                "hi there".to_string(),
            )
            .await;

        assert!(result.is_ok());

        // Verify the session history contains both messages
        let history = session_runner
            .get_history(&test_id)
            .await
            .expect("Failed to get history");

        assert_eq!(history.messages.len(), 2);
        assert_eq!(history.messages[0].role, ConversationRole::User);
        assert_eq!(history.messages[0].content, "hello world");
        assert_eq!(history.messages[1].role, ConversationRole::Assistant);
        assert_eq!(history.messages[1].content, "hi there");
    }

    #[tokio::test]
    async fn test_record_exchange_not_found() {
        let runner = Arc::new(Mutex::new(PipelineRunner::new()));
        let session_runner = SessionRunner::new(runner);

        let bogus_id = SessionId::new();
        let result = session_runner
            .record_exchange(
                &bogus_id,
                "hello".to_string(),
                "hi".to_string(),
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(SessionError::NotFound(_)) => (),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_record_exchange_closed_session() {
        let runner = Arc::new(Mutex::new(PipelineRunner::new()));
        let session_runner = SessionRunner::new(runner);

        let mut session = Session::new("test_agent", SessionPolicy::default());
        let test_id = session.id.clone();
        session.status = SessionStatus::Closed;
        {
            let mut sessions = session_runner.sessions.lock().await;
            sessions.insert(test_id.clone(), session);
        }

        let result = session_runner
            .record_exchange(
                &test_id,
                "hello".to_string(),
                "hi".to_string(),
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(SessionError::Closed) => (),
            _ => panic!("Expected Closed error"),
        }
    }
}
