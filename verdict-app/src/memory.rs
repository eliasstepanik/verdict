//! Memory store integration for verdict-app
//!
//! Provides a pre-configured in-memory store backed by InMemoryStore.

use std::sync::Arc;
use verdict_memory::InMemoryStore;
use verdict::memory::MemoryStore;

/// Build a pre-configured in-memory memory store for the app
pub fn build_memory_store() -> Arc<dyn MemoryStore> {
    Arc::new(InMemoryStore::new())
}
