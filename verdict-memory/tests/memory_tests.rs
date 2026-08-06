use serde_json::json;
use std::sync::Arc;
use verdict::memory::{MemoryMessage, MemoryRole, MemoryStore};
use verdict_memory::{
    InMemoryStore, ObservationalMemory, SemanticMemory, ThreadMemory, WorkingMemory,
};

#[tokio::test]
async fn test_in_memory_save_get_thread() {
    let store = Arc::new(InMemoryStore::new());

    // Save 5 messages
    for i in 0..5 {
        let msg = MemoryMessage::new(
            "thread1".to_string(),
            "resource1".to_string(),
            if i % 2 == 0 {
                MemoryRole::User
            } else {
                MemoryRole::Assistant
            },
            format!("Message {}", i),
        );
        store
            .save_message("thread1", "resource1", msg)
            .await
            .unwrap();
    }

    // Get all messages
    let messages = store.get_thread("thread1", None).await.unwrap();
    assert_eq!(messages.len(), 5);
    assert_eq!(messages[0].content, "Message 0");
    assert_eq!(messages[4].content, "Message 4");
}

#[tokio::test]
async fn test_in_memory_last_n_limit() {
    let store = Arc::new(InMemoryStore::new());

    // Save 10 messages
    for i in 0..10 {
        let msg = MemoryMessage::new(
            "thread2".to_string(),
            "resource2".to_string(),
            MemoryRole::User,
            format!("Message {}", i),
        );
        store
            .save_message("thread2", "resource2", msg)
            .await
            .unwrap();
    }

    // Get last 3 messages
    let messages = store.get_thread("thread2", Some(3)).await.unwrap();
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].content, "Message 7");
    assert_eq!(messages[1].content, "Message 8");
    assert_eq!(messages[2].content, "Message 9");
}

#[tokio::test]
async fn test_in_memory_working_memory() {
    let store = Arc::new(InMemoryStore::new());

    let data = json!({
        "user_id": 123,
        "state": "active",
        "tags": ["tag1", "tag2"]
    });

    store
        .save_working_memory("resource1", data.clone())
        .await
        .unwrap();
    let retrieved = store.get_working_memory("resource1").await.unwrap();

    assert_eq!(retrieved, Some(data));
}

#[tokio::test]
async fn test_in_memory_semantic_search() {
    let store = Arc::new(InMemoryStore::new());

    // Upsert 3 embeddings
    let embed1 = vec![1.0, 0.0, 0.0];
    let embed2 = vec![1.0, 0.1, 0.0];
    let embed3 = vec![0.0, 1.0, 0.0];

    store
        .upsert_embedding("id1", "text1", embed1.clone(), json!({"type": "A"}))
        .await
        .unwrap();
    store
        .upsert_embedding("id2", "text2", embed2.clone(), json!({"type": "A"}))
        .await
        .unwrap();
    store
        .upsert_embedding("id3", "text3", embed3.clone(), json!({"type": "B"}))
        .await
        .unwrap();

    // Search with embed1 query
    let results = store.search_semantic(embed1, 2).await.unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].id, "id1");
    assert!(results[0].score > 0.99); // Nearly identical
    assert_eq!(results[1].id, "id2");
}

#[tokio::test]
async fn test_in_memory_observations() {
    let store = Arc::new(InMemoryStore::new());

    // Save 3 observations
    store
        .save_observation("thread1", "Observation 1")
        .await
        .unwrap();
    store
        .save_observation("thread1", "Observation 2")
        .await
        .unwrap();
    store
        .save_observation("thread1", "Observation 3")
        .await
        .unwrap();

    // Get all observations
    let observations = store.get_observations("thread1").await.unwrap();
    assert_eq!(observations.len(), 3);
    assert_eq!(observations[0], "Observation 1");
    assert_eq!(observations[2], "Observation 3");
}

#[tokio::test]
async fn test_thread_memory_wrapper() {
    let store = Arc::new(InMemoryStore::new());
    let thread_mem = ThreadMemory::new(store);

    thread_mem
        .push("t1", "r1", MemoryRole::User, "Hello".to_string())
        .await
        .unwrap();
    thread_mem
        .push("t1", "r1", MemoryRole::Assistant, "Hi there".to_string())
        .await
        .unwrap();

    let messages = thread_mem.get("t1").await.unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "Hello");
}

#[tokio::test]
async fn test_working_memory_wrapper() {
    let store = Arc::new(InMemoryStore::new());
    let working_mem = WorkingMemory::new(store);

    let data = json!({"key": "value"});
    working_mem.set("r1", data.clone()).await.unwrap();

    let retrieved = working_mem.get("r1").await.unwrap();
    assert_eq!(retrieved, Some(data));
}

#[tokio::test]
async fn test_semantic_memory_wrapper() {
    let store = Arc::new(InMemoryStore::new());
    let semantic_mem = SemanticMemory::new(store);

    let embedding = vec![1.0, 0.0];
    semantic_mem
        .upsert("id1", "text", embedding.clone(), json!({}))
        .await
        .unwrap();

    let results = semantic_mem.search(embedding, 1).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "id1");
}

#[tokio::test]
async fn test_observational_memory_wrapper() {
    let store = Arc::new(InMemoryStore::new());
    let obs_mem = ObservationalMemory::new(store);

    obs_mem.save("t1", "Summary 1").await.unwrap();
    obs_mem.save("t1", "Summary 2").await.unwrap();

    let observations = obs_mem.get("t1").await.unwrap();
    assert_eq!(observations.len(), 2);
}
