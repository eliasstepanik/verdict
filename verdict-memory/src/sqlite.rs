/// SQLite-backed implementation of MemoryStore
///
/// This implementation uses SQLite for persistent storage of all memory types.
use async_trait::async_trait;
use chrono::Utc;
use rusqlite::{params, Connection, Result as SqlResult};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Mutex;

use verdict::memory::{MemoryError, MemoryMessage, MemoryRole, MemoryStore, SemanticResult};

/// SQLite memory store implementation
#[derive(Debug)]
pub struct SqliteMemoryStore {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteMemoryStore {
    pub fn new(db_path: &str) -> Result<Self, MemoryError> {
        let conn = Connection::open(db_path).map_err(|e| MemoryError::Backend(e.to_string()))?;

        // Create tables
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL,
                resource_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                metadata TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS working_memory (
                resource_id TEXT PRIMARY KEY,
                data TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS embeddings (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                embedding BLOB NOT NULL,
                metadata TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS observations (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                observation TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_messages_thread_id ON messages(thread_id);
            CREATE INDEX IF NOT EXISTS idx_observations_thread_id ON observations(thread_id);
            ",
        )
        .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl MemoryStore for SqliteMemoryStore {
    async fn save_message(
        &self,
        thread_id: &str,
        resource_id: &str,
        msg: MemoryMessage,
    ) -> Result<(), MemoryError> {
        let conn = self.conn.lock().await;
        let role_str = match msg.role {
            MemoryRole::User => "User",
            MemoryRole::Assistant => "Assistant",
            MemoryRole::System => "System",
            MemoryRole::Tool => "Tool",
        };

        let metadata_str = serde_json::to_string(&msg.metadata)
            .map_err(|e| MemoryError::Serialization(e.to_string()))?;

        conn.execute(
            "INSERT INTO messages (id, thread_id, resource_id, role, content, timestamp, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                msg.id,
                thread_id,
                resource_id,
                role_str,
                msg.content,
                msg.timestamp.to_rfc3339(),
                metadata_str,
            ],
        )
        .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn get_thread(
        &self,
        thread_id: &str,
        last_n: Option<usize>,
    ) -> Result<Vec<MemoryMessage>, MemoryError> {
        let conn = self.conn.lock().await;

        let query = if let Some(n) = last_n {
            format!(
                "SELECT id, thread_id, resource_id, role, content, timestamp, metadata
                 FROM messages WHERE thread_id = ?1
                 ORDER BY timestamp ASC LIMIT {} OFFSET (SELECT COUNT(*) FROM messages WHERE thread_id = ?1) - {}",
                n, n
            )
        } else {
            "SELECT id, thread_id, resource_id, role, content, timestamp, metadata
             FROM messages WHERE thread_id = ?1
             ORDER BY timestamp ASC"
                .to_string()
        };

        let mut stmt = conn
            .prepare(&query)
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        let messages = stmt
            .query_map(params![thread_id], |row| {
                let role_str: String = row.get(3)?;
                let role = match role_str.as_str() {
                    "User" => MemoryRole::User,
                    "Assistant" => MemoryRole::Assistant,
                    "System" => MemoryRole::System,
                    "Tool" => MemoryRole::Tool,
                    _ => MemoryRole::User,
                };

                let timestamp_str: String = row.get(5)?;
                let timestamp = chrono::DateTime::parse_from_rfc3339(&timestamp_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                let metadata_str: String = row.get(6)?;
                let metadata = serde_json::from_str(&metadata_str).unwrap_or(Value::Null);

                Ok(MemoryMessage {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    resource_id: row.get(2)?,
                    role,
                    content: row.get(4)?,
                    timestamp,
                    metadata,
                })
            })
            .map_err(|e| MemoryError::Backend(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(messages)
    }

    async fn save_working_memory(&self, resource_id: &str, data: Value) -> Result<(), MemoryError> {
        let conn = self.conn.lock().await;
        let data_str =
            serde_json::to_string(&data).map_err(|e| MemoryError::Serialization(e.to_string()))?;

        conn.execute(
            "INSERT OR REPLACE INTO working_memory (resource_id, data) VALUES (?1, ?2)",
            params![resource_id, data_str],
        )
        .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn get_working_memory(&self, resource_id: &str) -> Result<Option<Value>, MemoryError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn
            .prepare("SELECT data FROM working_memory WHERE resource_id = ?1")
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        let result = stmt
            .query_row(params![resource_id], |row| {
                let data_str: String = row.get(0)?;
                Ok(serde_json::from_str(&data_str).unwrap_or(Value::Null))
            })
            .ok();

        Ok(result)
    }

    async fn upsert_embedding(
        &self,
        id: &str,
        text: &str,
        embedding: Vec<f32>,
        metadata: Value,
    ) -> Result<(), MemoryError> {
        let conn = self.conn.lock().await;
        let embedding_bytes = embedding
            .iter()
            .flat_map(|f| f.to_le_bytes())
            .collect::<Vec<_>>();
        let metadata_str = serde_json::to_string(&metadata)
            .map_err(|e| MemoryError::Serialization(e.to_string()))?;

        conn.execute(
            "INSERT OR REPLACE INTO embeddings (id, text, embedding, metadata) VALUES (?1, ?2, ?3, ?4)",
            params![id, text, embedding_bytes, metadata_str],
        )
        .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn search_semantic(
        &self,
        query_embedding: Vec<f32>,
        top_k: usize,
    ) -> Result<Vec<SemanticResult>, MemoryError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn
            .prepare("SELECT id, text, embedding, metadata FROM embeddings")
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        let mut results: Vec<(String, f32)> = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let text: String = row.get(1)?;
                let embedding_bytes: Vec<u8> = row.get(2)?;
                let metadata_str: String = row.get(3)?;

                let embedding: Vec<f32> = embedding_bytes
                    .chunks(4)
                    .map(|chunk| {
                        let mut bytes = [0u8; 4];
                        bytes.copy_from_slice(chunk);
                        f32::from_le_bytes(bytes)
                    })
                    .collect();

                let score = cosine_similarity(&query_embedding, &embedding);
                let metadata = serde_json::from_str(&metadata_str).unwrap_or(Value::Null);

                Ok((id, score, text, metadata))
            })
            .map_err(|e| MemoryError::Backend(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Backend(e.to_string()))?
            .into_iter()
            .map(|(id, score, text, metadata)| ((id, score), (text, metadata)))
            .collect::<Vec<_>>()
            .into_iter()
            .map(|((id, score), (_, _))| (id, score))
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);

        // Second pass to get text and metadata
        let mut stmt = conn
            .prepare("SELECT id, text, embedding, metadata FROM embeddings WHERE id = ?1")
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        let semantic_results = results
            .into_iter()
            .map(|(id, score)| {
                let res = stmt.query_row(params![&id], |row| {
                    let text: String = row.get(1)?;
                    let metadata_str: String = row.get(3)?;
                    let metadata = serde_json::from_str(&metadata_str).unwrap_or(Value::Null);
                    Ok((text, metadata))
                });

                match res {
                    Ok((text, metadata)) => SemanticResult {
                        id,
                        text,
                        score,
                        metadata,
                    },
                    Err(_) => SemanticResult {
                        id,
                        text: String::new(),
                        score,
                        metadata: Value::Null,
                    },
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
        let conn = self.conn.lock().await;

        conn.execute(
            "INSERT INTO observations (thread_id, observation, timestamp) VALUES (?1, ?2, ?3)",
            params![thread_id, observation, Utc::now().to_rfc3339()],
        )
        .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(())
    }

    async fn get_observations(&self, thread_id: &str) -> Result<Vec<String>, MemoryError> {
        let conn = self.conn.lock().await;

        let mut stmt = conn
            .prepare(
                "SELECT observation FROM observations WHERE thread_id = ?1 ORDER BY timestamp ASC",
            )
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        let observations = stmt
            .query_map(params![thread_id], |row| row.get(0))
            .map_err(|e| MemoryError::Backend(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| MemoryError::Backend(e.to_string()))?;

        Ok(observations)
    }
}

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
