use serde_json::Value;
/// SemanticMemory: Cosine similarity over embeddings
///
/// Provides upsert and search operations for embeddings
use std::sync::Arc;
use verdict::memory::{MemoryError, MemoryStore, SemanticResult};

pub struct SemanticMemory {
    store: Arc<dyn MemoryStore>,
}

impl SemanticMemory {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }

    /// Upsert an embedding
    pub async fn upsert(
        &self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> Result<(), MemoryError> {
        self.store
            .upsert_embedding(id, text, embedding, metadata)
            .await
    }

    /// Search by embedding similarity
    pub async fn search(
        &self,
        query_embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<SemanticResult>, MemoryError> {
        self.store.search_semantic(query_embedding, top_k).await
    }
}
