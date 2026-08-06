//! LLM provider integration.

pub mod client;
pub mod provider;

pub use client::LlmClient;
pub use provider::{
    ChatMessage, ChatRole, ConversationRegistry, LlmChunk, LlmError, LlmProvider, LlmRequest,
    LlmResponse, LlmUsage, MessageHistory, OpenAiCompatibleProvider, ToolCall, ToolSchema,
};

// Re-export ProviderSpec from action module (moved there to resolve conflict)
pub use crate::action::ProviderSpec;
