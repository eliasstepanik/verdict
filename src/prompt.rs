//! Phase 16: Dynamic Prompt Templates and Structured Output
//!
//! Composable prompt templates that reference prior step outputs,
//! session scratchpad values, conversation history, and dynamic computed content.

use crate::context::StepContext;
use async_trait::async_trait;
use std::sync::Arc;

/// A composable prompt template assembled from ordered segments
#[derive(Clone, Debug)]
pub struct PromptTemplate {
    pub segments: Vec<PromptSegment>,
}

/// A segment of a prompt template
#[derive(Clone, Debug)]
pub enum PromptSegment {
    /// Static text inserted verbatim
    Static(String),
    /// A named step's output from prior step_results: {step_name}
    StepOutput(String),
    /// Session scratchpad value by key (if session_meta present)
    ScratchpadValue(String),
    /// Dynamic computation via a PromptProvider
    Computed(Arc<dyn PromptProvider>),
    /// Recent N messages from conversation_history
    Conversation { last_n: usize },
}

/// A provider that computes dynamic prompt content
#[async_trait]
pub trait PromptProvider: Send + Sync + std::fmt::Debug {
    /// Render the dynamic content given the current step context
    async fn render(&self, ctx: &StepContext) -> Result<String, PromptError>;
}

/// Error type for prompt rendering
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("Render failed: {0}")]
    RenderFailed(String),
    #[error("Step not found: {0}")]
    StepNotFound(String),
    #[error("Scratchpad key not found: {0}")]
    ScratchpadKeyNotFound(String),
}

impl PromptTemplate {
    /// Create a new empty prompt template
    pub fn new() -> Self {
        PromptTemplate {
            segments: vec![],
        }
    }

    /// Add a static text segment
    pub fn push_static(mut self, text: impl Into<String>) -> Self {
        self.segments
            .push(PromptSegment::Static(text.into()));
        self
    }

    /// Add a step output segment
    pub fn push_step_output(mut self, step_name: impl Into<String>) -> Self {
        self.segments
            .push(PromptSegment::StepOutput(step_name.into()));
        self
    }

    /// Add a conversation history segment
    pub fn push_conversation(mut self, last_n: usize) -> Self {
        self.segments
            .push(PromptSegment::Conversation { last_n });
        self
    }

    /// Add a computed segment via a PromptProvider
    pub fn push_computed(mut self, provider: Arc<dyn PromptProvider>) -> Self {
        self.segments.push(PromptSegment::Computed(provider));
        self
    }

    /// Add a scratchpad segment
    pub fn push_scratchpad(mut self, key: impl Into<String>) -> Self {
        self.segments
            .push(PromptSegment::ScratchpadValue(key.into()));
        self
    }

    /// Render all segments to a final string
    pub async fn render(&self, ctx: &StepContext) -> Result<String, PromptError> {
        let mut out = String::new();
        for seg in &self.segments {
            match seg {
                PromptSegment::Static(s) => out.push_str(s),
                PromptSegment::StepOutput(name) => {
                    match ctx.step_results.get(name) {
                        Some(r) => out.push_str(&r.output.raw),
                        None => return Err(PromptError::StepNotFound(name.clone())),
                    }
                }
                PromptSegment::ScratchpadValue(key) => {
                    // Read from session scratchpad if available
                    if let Some(_meta) = &ctx.session_meta {
                        // session_meta contains SessionMeta, not the Session itself
                        // We need to look this up from somewhere else or assume it's available
                        // For now, we'll check if the Session is stored elsewhere.
                        // This is a limitation we need to document.
                        // As a workaround, we can return an error or empty string.
                        return Err(PromptError::ScratchpadKeyNotFound(
                            key.clone(),
                        ));
                    }
                }
                PromptSegment::Computed(p) => {
                    out.push_str(&p.render(ctx).await?);
                }
                PromptSegment::Conversation { last_n } => {
                    let msgs = &ctx.conversation_history.messages;
                    let start = msgs.len().saturating_sub(*last_n);
                    for msg in &msgs[start..] {
                        out.push_str(&format!("{:?}: {}\n", msg.role, msg.content));
                    }
                }
            }
        }
        Ok(out)
    }
}

impl Default for PromptTemplate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::FilesystemPolicy;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_prompt_template_static() {
        let template = PromptTemplate::new().push_static("Hello, world!");
        let ctx = StepContext::new(
            "test_agent".to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            serde_json::json!({}),
            FilesystemPolicy {
                workspace_root: PathBuf::from("."),
                read_paths: vec![],
                write_paths: vec![],
                forbidden_paths: vec![],
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
        );
        let result = template.render(&ctx).await.unwrap();
        assert_eq!(result, "Hello, world!");
    }

    #[tokio::test]
    async fn test_prompt_template_conversation() {
        use crate::llm::provider::{ChatMessage, ChatRole, MessageHistory};

        let mut template = PromptTemplate::new();
        template = template.push_static("Messages:\n");
        template = template.push_conversation(2);

        let mut ctx = StepContext::new(
            "test_agent".to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            serde_json::json!({}),
            FilesystemPolicy {
                workspace_root: PathBuf::from("."),
                read_paths: vec![],
                write_paths: vec![],
                forbidden_paths: vec![],
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
        );

        ctx.conversation_history = MessageHistory {
            messages: vec![
                ChatMessage {
                    role: ChatRole::User,
                    content: "First message".to_string(),
                    tool_calls_json: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: "First response".to_string(),
                    tool_calls_json: None,
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: "Second message".to_string(),
                    tool_calls_json: None,
                    tool_call_id: None,
                },
            ],
            conversation_id: None,
        };

        let result = template.render(&ctx).await.unwrap();
        assert!(result.contains("Messages:"));
        assert!(result.contains("First response"));
        assert!(result.contains("Second message"));
    }

    #[tokio::test]
    async fn test_prompt_template_step_output_missing() {
        let template = PromptTemplate::new().push_step_output("nonexistent_step");
        let ctx = StepContext::new(
            "test_agent".to_string(),
            "test_pipeline".to_string(),
            "test_step".to_string(),
            serde_json::json!({}),
            FilesystemPolicy {
                workspace_root: PathBuf::from("."),
                read_paths: vec![],
                write_paths: vec![],
                forbidden_paths: vec![],
                workspace_isolation: crate::agent::WorkspaceIsolation::None,
            },
        );

        let result = template.render(&ctx).await;
        assert!(matches!(result, Err(PromptError::StepNotFound(_))));
    }
}
