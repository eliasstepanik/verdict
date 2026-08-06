/// In-memory implementation of MemoryStore
///
/// This implementation uses HashMap for all storage, suitable for testing,
/// development, and applications that don't require persistence.
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use verdict::memory::{MemoryError, MemoryMessage, MemoryStore, SemanticResult};

/// In-memory memory store implementation
#[derive(Debug)]
pub struct InMemoryStore {
    threads: Arc<Mutex<HashMap<String, Vec<MemoryMessage>>>>,
    working_memory: Arc<Mutex<HashMap<String, Value>>>,
    embeddings: Arc<Mutex<HashMap<String, EmbeddingEntry>>>,
    observations: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

#[derive(Debug)]
struct EmbeddingEntry {
    text: String,
    embedding: Vec<f32>,
    metadata: Value,
}

impl InMemoryStore {
    pub fn new() -> Self {
        Self {
            threads: Arc::new(Mutex::new(HashMap::new())),
            working_memory: Arc::new(Mutex::new(HashMap::new())),
            embeddings: Arc::new(Mutex::new(HashMap::new())),
            observations: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl Default for InMemoryStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryStore for InMemoryStore {
    async fn save_message(
        &self,
        thread_id: &str,
        _resource_id: &str,
        msg: MemoryMessage,
    ) -> Result<(), MemoryError> {
        let mut threads = self.threads.lock().await;
        threads
            .entry(thread_id.to_string())
            .or_insert_with(Vec::new)
            .push(msg);
        Ok(())
    }

    async fn get_thread(
        &self,
        thread_id: &str,
        last_n: Option<usize>,
    ) -> Result<Vec<MemoryMessage>, MemoryError> {
        let threads = self.threads.lock().await;
        match threads.get(thread_id) {
            Some(messages) => {
                let result = if let Some(n) = last_n {
                    let skip = messages.len().saturating_sub(n);
                    messages[skip..].to_vec()
                } else {
                    messages.clone()
                };
                Ok(result)
            }
            None => Ok(vec![]),
        }
    }

    async fn save_working_memory(&self, resource_id: &str, data: Value) -> Result<(), MemoryError> {
        let mut working = self.working_memory.lock().await;
        working.insert(resource_id.to_string(), data);
        Ok(())
    }

    async fn get_working_memory(&self, resource_id: &str) -> Result<Option<Value>, MemoryError> {
        let working = self.working_memory.lock().await;
        Ok(working.get(resource_id).cloned())
    }

    async fn upsert_embedding(
        &self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> Result<(), MemoryError> {
        let mut embeddings = self.embeddings.lock().await;
        embeddings.insert(
            id.to_string(),
            EmbeddingEntry {
                text: text.to_string(),
                embedding,
                metadata,
            },
        );
        Ok(())
    }

    async fn search_semantic(
        &self,
        query_embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<SemanticResult>, MemoryError> {
        let embeddings = self.embeddings.lock().await;

        let mut results: Vec<(String, f32)> = embeddings
            .iter()
            .map(|(id, entry)| {
                let score = cosine_similarity(&query_embedding, &entry.embedding);
                (id.clone(), score)
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);

        let semantic_results = results
            .into_iter()
            .map(|(id, score)| {
                let entry = embeddings.get(&id).unwrap();
                SemanticResult {
                    id,
                    text: entry.text.clone(),
                    score,
                    metadata: entry.metadata.clone(),
                }
            })
            .collect();

        Ok(semantic_results)
    }

    async fn save_observation(
        &self,
        thread_id: &str,
        observation: &str,
    ) -> Result<(), MemoryError> {
        let mut observations = self.observations.lock().await;
        observations
            .entry(thread_id.to_string())
            .or_insert_with(Vec::new)
            .push(observation.to_string());
        Ok(())
    }

    async fn get_observations(&self, thread_id: &str) -> Result<Vec<String>, MemoryError> {
        let observations = self.observations.lock().await;
        Ok(observations.get(thread_id).cloned().unwrap_or_default())
    }
}

/// Compute cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.is_empty() || b.is_empty() || a.len() != b.len() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }

    dot_product / (magnitude_a * magnitude_b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.0001);

        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 0.0001);
    }
}
