//! Verdict Memory System - Multi-tier memory for agents
//!
//! This crate provides implementations of memory stores and memory tiers
//! for the Verdict framework, enabling agents to persist and retrieve
//! conversation history, working memory, semantic embeddings, and observations.

pub mod in_memory;
pub mod tiers;
pub mod tools;

#[cfg(feature = "sqlite")]
pub mod sqlite;

// Re-export main types from verdict for convenience
pub use verdict::memory::{MemoryStore, MemoryMessage, MemoryRole, SemanticResult, MemoryError};

// Re-export implementations
pub use in_memory::InMemoryStore;
pub use tiers::{ThreadMemory, WorkingMemory, SemanticMemory, ObservationalMemory};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteMemoryStore;
