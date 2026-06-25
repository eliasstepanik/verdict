/// WorkingMemory: Structured JSON state per resource
///
/// Provides get/set access to structured working memory

use std::sync::Arc;
use serde_json::Value;
use verdict::memory::{MemoryStore, MemoryError};

pub struct WorkingMemory {
    store: Arc<dyn MemoryStore>,
}

impl WorkingMemory {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }

    /// Set working memory for a resource
    pub async fn set(&self, resource_id: &str, value: Value) -> Result<(), MemoryError> {
        self.store.save_working_memory(resource_id, value).await
    }

    /// Get working memory for a resource
    pub async fn get(&self, resource_id: &str) -> Result<Option<Value>, MemoryError> {
        self.store.get_working_memory(resource_id).await
    }
}
