/// Memory tools - built-in tools for memory operations
///
/// These tools are auto-registered when PipelineRunner has a memory store.

use std::sync::Arc;
use serde_json::{json, Value};
use verdict::tools::{Tool, ToolOutput, ToolError, ToolContext, ToolSource};
use verdict::memory::MemoryStore;
use async_trait::async_trait;

/// Tool for retrieving thread history
pub struct MemoryGetThreadTool {
    store: Arc<dyn MemoryStore>,
}

impl MemoryGetThreadTool {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryGetThreadTool {
    fn name(&self) -> &str {
        "memory.get_thread"
    }

    fn description(&self) -> &str {
        "Retrieve conversation history from a thread"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["thread_id"],
            "properties": {
                "thread_id": {
                    "type": "string",
                    "description": "The thread ID to retrieve"
                },
                "last_n": {
                    "type": "number",
                    "description": "Optional: limit to last N messages"
                }
            }
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let thread_id = args["thread_id"]
            .as_str()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "thread_id is required".to_string() })?;

        let last_n = args["last_n"].as_u64().map(|n| n as usize);

        let messages = self
            .store
            .get_thread(thread_id, last_n)
            .await
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        let output = serde_json::to_string(&messages)
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        Ok(ToolOutput::text(output))
    }

}

/// Tool for searching semantic memory
pub struct MemorySearchTool {
    store: Arc<dyn MemoryStore>,
}

impl MemorySearchTool {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemorySearchTool {
    fn name(&self) -> &str {
        "memory.search"
    }

    fn description(&self) -> &str {
        "Search semantic memory by embedding similarity"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["embedding", "top_k"],
            "properties": {
                "embedding": {
                    "type": "array",
                    "items": {"type": "number"},
                    "description": "Query embedding vector"
                },
                "top_k": {
                    "type": "number",
                    "description": "Number of top results to return"
                }
            }
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let embedding = args["embedding"]
            .as_array()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "embedding must be an array".to_string() })?
            .iter()
            .map(|v| v.as_f64().ok_or_else(|| ToolError::SchemaValidationFailed { reason: "embedding values must be numbers".to_string() }))
            .collect::<Result<Vec<f64>, ToolError>>()?
            .into_iter()
            .map(|v| v as f32)
            .collect();

        let top_k = args["top_k"]
            .as_u64()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "top_k is required".to_string() })?
            as usize;

        let results = self
            .store
            .search_semantic(embedding, top_k)
            .await
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        let output = serde_json::to_string(&results)
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        Ok(ToolOutput::text(output))
    }
}

/// Tool for saving working memory
pub struct MemorySetWorkingTool {
    store: Arc<dyn MemoryStore>,
}

impl MemorySetWorkingTool {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemorySetWorkingTool {
    fn name(&self) -> &str {
        "memory.set_working"
    }

    fn description(&self) -> &str {
        "Save structured JSON working memory"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["resource_id", "data"],
            "properties": {
                "resource_id": {
                    "type": "string",
                    "description": "Resource identifier"
                },
                "data": {
                    "description": "Structured data to store"
                }
            }
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let resource_id = args["resource_id"]
            .as_str()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "resource_id is required".to_string() })?;

        let data = args["data"].clone();

        self.store
            .save_working_memory(resource_id, data)
            .await
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        Ok(ToolOutput::text("OK".to_string()))
    }

    async fn call_streaming(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> Result<Vec<verdict::tools::ToolChunk>, ToolError> {
        let output = self.call(args, ctx).await?;
        Ok(vec![verdict::tools::ToolChunk {
            delta: output.raw,
            is_final: true,
        }])
    }
}

/// Tool for retrieving working memory
pub struct MemoryGetWorkingTool {
    store: Arc<dyn MemoryStore>,
}

impl MemoryGetWorkingTool {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemoryGetWorkingTool {
    fn name(&self) -> &str {
        "memory.get_working"
    }

    fn description(&self) -> &str {
        "Retrieve structured JSON working memory"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["resource_id"],
            "properties": {
                "resource_id": {
                    "type": "string",
                    "description": "Resource identifier"
                }
            }
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let resource_id = args["resource_id"]
            .as_str()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "resource_id is required".to_string() })?;

        let data = self
            .store
            .get_working_memory(resource_id)
            .await
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        let output = serde_json::to_string(&data)
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        Ok(ToolOutput::text(output))
    }

}

/// Tool for saving a message to thread memory
pub struct MemorySaveMessageTool {
    store: Arc<dyn MemoryStore>,
}

impl MemorySaveMessageTool {
    pub fn new(store: Arc<dyn MemoryStore>) -> Self {
        Self { store }
    }
}

#[async_trait]
impl Tool for MemorySaveMessageTool {
    fn name(&self) -> &str {
        "memory.save_message"
    }

    fn description(&self) -> &str {
        "Save a message to thread memory"
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["thread_id", "resource_id", "role", "content"],
            "properties": {
                "thread_id": {
                    "type": "string",
                    "description": "Thread identifier"
                },
                "resource_id": {
                    "type": "string",
                    "description": "Resource identifier"
                },
                "role": {
                    "type": "string",
                    "enum": ["User", "Assistant", "System", "Tool"],
                    "description": "Message role"
                },
                "content": {
                    "type": "string",
                    "description": "Message content"
                }
            }
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let thread_id = args["thread_id"]
            .as_str()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "thread_id is required".to_string() })?;

        let resource_id = args["resource_id"]
            .as_str()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "resource_id is required".to_string() })?;

        let role_str = args["role"]
            .as_str()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "role is required".to_string() })?;

        let content = args["content"]
            .as_str()
            .ok_or_else(|| ToolError::SchemaValidationFailed { reason: "content is required".to_string() })?;

        let role = match role_str {
            "User" => verdict::memory::MemoryRole::User,
            "Assistant" => verdict::memory::MemoryRole::Assistant,
            "System" => verdict::memory::MemoryRole::System,
            "Tool" => verdict::memory::MemoryRole::Tool,
            _ => return Err(ToolError::SchemaValidationFailed { reason: format!("Invalid role: {}", role_str) }),
        };

        let msg = verdict::memory::MemoryMessage::new(
            thread_id.to_string(),
            resource_id.to_string(),
            role,
            content.to_string(),
        );

        self.store
            .save_message(thread_id, resource_id, msg)
            .await
            .map_err(|e| ToolError::ExecutionFailed { reason: e.to_string() })?;

        Ok(ToolOutput::text("OK".to_string()))
    }

}