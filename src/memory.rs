/// Multi-tier memory system for Verdict agents
///
/// This module defines the core traits and types for the memory system,
/// which are then implemented in the verdict-memory crate.
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Debug;

/// The core trait for all memory backends
#[async_trait]
pub trait MemoryStore: Send + Sync + Debug {
    /// Save a message to a thread
    async fn save_message(
        &self,
        thread_id: &str,
        resource_id: &str,
        msg: MemoryMessage,
    ) -> Result<(), MemoryError>;

    /// Get messages from a thread, optionally limiting to last N
    async fn get_thread(
        &self,
        thread_id: &str,
        last_n: Option<usize>,
    ) -> Result<Vec<MemoryMessage>, MemoryError>;

    /// Save structured JSON working memory for a resource
    async fn save_working_memory(&self, resource_id: &str, data: Value) -> Result<(), MemoryError>;

    /// Retrieve structured working memory for a resource
    async fn get_working_memory(&self, resource_id: &str) -> Result<Option<Value>, MemoryError>;

    /// Upsert an embedding with semantic metadata
    async fn upsert_embedding(
        &self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> Result<(), MemoryError>;

    /// Search embeddings by cosine similarity
    async fn search_semantic(
        &self,
        query_embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<SemanticResult>, MemoryError>;

    /// Save an observation (LLM-compressed summary)
    async fn save_observation(&self, thread_id: &str, observation: &str)
        -> Result<(), MemoryError>;

    /// Retrieve observations for a thread
    async fn get_observations(&self, thread_id: &str) -> Result<Vec<String>, MemoryError>;
}

/// A message in thread memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryMessage {
    pub id: String,
    pub thread_id: String,
    pub resource_id: String,
    pub role: MemoryRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub metadata: Value,
}

/// The role of a message sender
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MemoryRole {
    User,
    Assistant,
    System,
    Tool,
}

/// Result of a semantic similarity search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticResult {
    pub id: String,
    pub text: String,
    pub score: f32,
    pub metadata: Value,
}

/// Errors from memory operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryError {
    Io(String),
    Serialization(String),
    NotFound(String),
    Backend(String),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::Io(msg) => write!(f, "IO error: {}", msg),
            MemoryError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            MemoryError::NotFound(msg) => write!(f, "Not found: {}", msg),
            MemoryError::Backend(msg) => write!(f, "Backend error: {}", msg),
        }
    }
}

impl std::error::Error for MemoryError {}

impl MemoryMessage {
    pub fn new(thread_id: String, resource_id: String, role: MemoryRole, content: String) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            thread_id,
            resource_id,
            role,
            content,
            timestamp: Utc::now(),
            metadata: Value::Null,
        }
    }
}
