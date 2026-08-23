//! Message types and conversation management for LLM interactions.

use serde::{Deserialize, Serialize};

/// Role of a message in a conversation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

/// A single message in a conversation history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    /// For assistant messages that invoked tools: the raw tool_calls JSON array
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls_json: Option<serde_json::Value>,
    /// For tool result messages: the tool_call_id this result corresponds to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn user(content: String) -> Self {
        Self {
            role: ChatRole::User,
            content,
            tool_calls_json: None,
            tool_call_id: None,
        }
    }
    pub fn assistant(content: String) -> Self {
        Self {
            role: ChatRole::Assistant,
            content,
            tool_calls_json: None,
            tool_call_id: None,
        }
    }
    pub fn assistant_with_tool_calls(content: String, tool_calls: serde_json::Value) -> Self {
        Self {
            role: ChatRole::Assistant,
            content,
            tool_calls_json: Some(tool_calls),
            tool_call_id: None,
        }
    }
    pub fn tool_result(tool_call_id: String, content: String) -> Self {
        Self {
            role: ChatRole::Tool,
            content,
            tool_calls_json: None,
            tool_call_id: Some(tool_call_id),
        }
    }
}

/// Conversation history for multi-turn LLM interactions
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MessageHistory {
    pub messages: Vec<ChatMessage>,
    pub conversation_id: Option<String>,
}

impl MessageHistory {
    /// Create a new empty message history
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a message to the history
    pub fn push(&mut self, role: ChatRole, content: String) {
        self.messages.push(ChatMessage {
            role,
            content,
            tool_calls_json: None,
            tool_call_id: None,
        });
    }

    /// Returns true if the history has no messages
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }
}

/// Registry for managing named conversations across multiple LLM calls.
/// Enables multi-turn conversations within a single pipeline run.
#[derive(Debug, Clone, Default)]
pub struct ConversationRegistry {
    /// Auto-generated titles for conversations (Phase A8)
    titles: std::collections::HashMap<String, String>,
    conversations: std::collections::HashMap<String, MessageHistory>,
}

impl ConversationRegistry {
    /// Create a new empty conversation registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Get or create a conversation by ID
    pub fn get_or_create(&mut self, id: &str) -> &mut MessageHistory {
        self.conversations
            .entry(id.to_string())
            .or_insert_with(MessageHistory::new)
    }

    /// Get a conversation by ID without creating it
    pub fn get(&self, id: &str) -> Option<&MessageHistory> {
        self.conversations.get(id)
    }

    /// Insert or replace a conversation
    pub fn insert(&mut self, id: String, history: MessageHistory) {
        self.conversations.insert(id, history);
    }

    /// Get the auto-generated title for a conversation (Phase A8)
    pub fn get_title(&self, id: &str) -> Option<&str> {
        self.titles.get(id).map(|s| s.as_str())
    }

    /// Set the auto-generated title for a conversation (Phase A8)
    pub fn set_title(&mut self, id: String, title: String) {
        self.titles.insert(id, title);
    }

    /// List all conversations as (id, history) pairs
    pub fn list_conversations(&self) -> Vec<(String, MessageHistory)> {
        self.conversations
            .iter()
            .map(|(id, history)| (id.clone(), history.clone()))
            .collect()
    }
}
