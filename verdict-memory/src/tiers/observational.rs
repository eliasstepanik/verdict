/// ObservationalMemory: LLM-compressed summaries
///
/// Provides save and retrieve operations for observations (summaries)
use std::sync::Arc;
use verdict::memory::{MemoryError, MemoryStore};

pub struct ObservationalMemory {
    store: Arc<dyn MemoryStore>,
}

impl ObservationalMemory {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }

    /// Save an observation for a thread
    pub async fn save(&self, thread_id: &str, observation: &str) -> Result<(), MemoryError> {
        self.store.save_observation(thread_id, observation).await
    }

    /// Get all observations for a thread
    pub async fn get(&self, thread_id: &str) -> Result<Vec<String>, MemoryError> {
        self.store.get_observations(thread_id).await
    }
}
