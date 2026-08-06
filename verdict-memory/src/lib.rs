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
pub use verdict::memory::{MemoryError, MemoryMessage, MemoryRole, MemoryStore, SemanticResult};

// Re-export implementations
pub use in_memory::InMemoryStore;
pub use tiers::{ObservationalMemory, SemanticMemory, ThreadMemory, WorkingMemory};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteMemoryStore;
