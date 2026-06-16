//! LLM provider integration.

pub mod provider;
pub mod client;


pub use provider::{
    LlmProvider, LlmRequest, LlmResponse, LlmUsage, LlmError, OpenAiCompatibleProvider,
    LlmChunk, ChatRole, ChatMessage, MessageHistory, ConversationRegistry, ToolSchema, ToolCall,
};
pub use client::LlmClient;


// Re-export ProviderSpec from action module (moved there to resolve conflict)
pub use crate::action::ProviderSpec;
