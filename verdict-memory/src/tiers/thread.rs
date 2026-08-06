/// ThreadMemory: Persisted conversation history
///
/// Provides access to thread-based conversation memory with optional limiting
use std::sync::Arc;
use verdict::memory::{MemoryError, MemoryMessage, MemoryRole, MemoryStore};

pub struct ThreadMemory {
    store: Arc<dyn MemoryStore>,
    last_n: Option<usize>,
}

impl ThreadMemory {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self {
            store,
            last_n: None,
        }
    }

    pub fn with_limit(store: Arc<dyn MemoryStore>, last_n: usize) -> Self {
        Self {
            store,
            last_n: Some(last_n),
        }
    }

    /// Push a message to a thread
    pub async fn push(
        &self,
        thread_id: &str,
        resource_id: &str,
        role: MemoryRole,
        content: String,
    ) -> Result<(), MemoryError> {
        let msg = MemoryMessage::new(
            thread_id.to_string(),
            resource_id.to_string(),
            role,
            content,
        );
        self.store.save_message(thread_id, resource_id, msg).await
    }

    /// Get all messages in a thread (respecting limit if set)
    pub async fn get(&self, thread_id: &str) -> Result<Vec<MemoryMessage>, MemoryError> {
        self.store.get_thread(thread_id, self.last_n).await
    }
}
