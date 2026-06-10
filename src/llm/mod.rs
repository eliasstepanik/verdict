//! LLM provider integration.

pub mod provider;
pub mod client;

pub use provider::{
    LlmProvider, LlmRequest, LlmResponse, LlmUsage,
    LlmError, ProviderSpec, OpenAiCompatibleProvider,
    LlmChunk, ChatRole, ChatMessage, MessageHistory, ConversationRegistry,
};
pub use client::LlmClient;
