/// Phase C1: Multi-Tier Memory System
///
/// Tests for memory integration with PipelineRunner and StepContext

use verdict::prelude::*;
use serde_json::json;
use std::sync::Arc;

// Mock memory store for testing
#[allow(dead_code)]
#[derive(Debug)]
struct MockMemoryStore;

#[async_trait::async_trait]
impl verdict::memory::MemoryStore for MockMemoryStore {
    async fn save_message(
        &self,
        _thread_id: &str,
        _resource_id: &str,
        _msg: verdict::memory::MemoryMessage,
    ) -> Result<(), verdict::memory::MemoryError> {
        Ok(())
    }

    async fn get_thread(
        &self,
        _thread_id: &str,
        _last_n: Option<usize>,
    ) -> Result<Vec<verdict::memory::MemoryMessage>, verdict::memory::MemoryError> {
        Ok(vec![])
    }

    async fn save_working_memory(
        &self,
        _resource_id: &str,
        _data: serde_json::Value,
    ) -> Result<(), verdict::memory::MemoryError> {
        Ok(())
    }

    async fn get_working_memory(
        &self,
        _resource_id: &str,
    ) -> Result<Option<serde_json::Value>, verdict::memory::MemoryError> {
        Ok(None)
    }

    async fn upsert_embedding(
        &self,
        _id: &str,
        _text: &str,
        _embedding: Vec<f32>,
        _metadata: serde_json::Value,
    ) -> Result<(), verdict::memory::MemoryError> {
        Ok(())
    }

    async fn search_semantic(
        &self,
        _query_embedding: Vec<f32>,
        _top_k: usize,
    ) -> Result<Vec<verdict::memory::SemanticResult>, verdict::memory::MemoryError> {
        Ok(vec![])
    }

    async fn save_observation(
        &self,
        _thread_id: &str,
        _observation: &str,
    ) -> Result<(), verdict::memory::MemoryError> {
        Ok(())
    }

    async fn get_observations(
        &self,
        _thread_id: &str,
    ) -> Result<Vec<String>, verdict::memory::MemoryError> {
        Ok(vec![])
    }
}

#[test]
fn test_runner_with_memory_field() {
    let runner = PipelineRunner::new();
    assert!(runner.memory.is_none());

    let memory_store = Arc::new(MockMemoryStore);
    let runner = runner.with_memory(memory_store.clone());
    assert!(runner.memory.is_some());
}

#[test]
fn test_step_context_has_memory_field() {
    let ctx = StepContext::new(
        "agent".to_string(),
        "pipeline".to_string(),
        "step".to_string(),
        json!({}),
        FilesystemPolicy::new(std::path::PathBuf::from(".")),
    );
    
    // Memory field should be None by default
    assert!(ctx.memory.is_none());
}

#[test]
fn test_step_context_memory_initialization() {
    let mut ctx = StepContext::new(
        "agent".to_string(),
        "pipeline".to_string(),
        "step".to_string(),
        json!({}),
        FilesystemPolicy::new(std::path::PathBuf::from(".")),
    );
    
    // Set memory
    let memory_store = Arc::new(MockMemoryStore);
    ctx.memory = Some(memory_store.clone());
    
    assert!(ctx.memory.is_some());
}

#[tokio::test]
async fn test_memory_message_creation() {
    let msg = MemoryMessage::new(
        "thread1".to_string(),
        "resource1".to_string(),
        MemoryRole::User,
        "Hello".to_string(),
    );

    assert_eq!(msg.thread_id, "thread1");
    assert_eq!(msg.resource_id, "resource1");
    assert_eq!(msg.role, MemoryRole::User);
    assert_eq!(msg.content, "Hello");
    assert!(!msg.id.is_empty());
}

#[test]
fn test_memory_role_enum() {
    let user = MemoryRole::User;
    let assistant = MemoryRole::Assistant;
    let system = MemoryRole::System;
    let tool = MemoryRole::Tool;

    assert_eq!(user, MemoryRole::User);
    assert_ne!(user, assistant);
    assert_ne!(assistant, system);
    assert_ne!(system, tool);
}

#[tokio::test]
async fn test_pipeline_runner_builder_chain() {
    let memory = Arc::new(MockMemoryStore);
    
    let runner = PipelineRunner::new()
        .with_memory(memory);

    assert!(runner.memory.is_some());
}
