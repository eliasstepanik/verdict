#![allow(dead_code)]

//! Core Tool trait and types

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::audit::AuditLog;
use crate::agent::{FilesystemPolicy, NetworkPolicy};
use crate::toolset::ToolSet;

/// Source of a tool
#[derive(Debug, Clone)]
pub enum ToolSource {
    /// Built-in Verdict tool
    Builtin,
    /// Local Rust function tool
    LocalFunction,
    /// Tool from an MCP server
    McpServer {
        server_name: String,
        tool_name: String,
    },
    /// External command-line tool
    ExternalCommand { command: String },
    /// HTTP/REST tool
    Http { base_url: String },
}

/// Error from tool execution
#[derive(Error, Debug, Clone)]
pub enum ToolError {
    #[error("tool '{tool}' not allowed")]
    NotAllowed { tool: String },

    #[error("tool '{tool}' not found")]
    NotFound { tool: String },

    #[error("schema validation failed: {reason}")]
    SchemaValidationFailed { reason: String },

    #[error("execution failed: {reason}")]
    ExecutionFailed { reason: String },

    #[error("I/O error: {0}")]
    IoError(String),
}

/// Severity level for diagnostics
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

/// A single diagnostic entry (error, warning, info, hint)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticEntry {
    pub severity: DiagnosticSeverity,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}

/// Structured output formats for tool results
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StructuredOutput {
    Text(String),
    Markdown(String),
    Code {
        language: String,
        source: String,
    },
    Diff {
        unified: String,
    },
    Diagnostics(Vec<DiagnosticEntry>),
    FileSnippet {
        path: String,
        start_line: u32,
        end_line: u32,
        content: String,
        language: Option<String>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Custom {
        schema: String,
        value: Value,
    },
}

/// Output from a tool call
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// Raw output as string
    pub raw: String,
    /// Optionally parsed JSON representation
    pub parsed: Option<Value>,
    /// Optionally structured output (Phase 16)
    pub structured: Option<StructuredOutput>,
}

/// A chunk of output from a streaming tool call
#[derive(Debug, Clone)]
pub struct ToolChunk {
    /// The incremental text delta in this chunk
    pub delta: String,
    /// Whether this is the final chunk
    pub is_final: bool,
}

impl ToolOutput {
    /// Create a text-only output
    pub fn text(raw: String) -> Self {
        Self {
            raw,
            parsed: None,
            structured: None,
        }
    }

    /// Create output from JSON value
    pub fn json(value: Value) -> Self {
        let raw = value.to_string();
        Self {
            raw,
            parsed: Some(value),
            structured: None,
        }
    }

    /// Create output with structured data
    pub fn with_structured(raw: String, structured: StructuredOutput) -> Self {
        Self {
            raw,
            parsed: None,
            structured: Some(structured),
        }
    }

    /// Get output as string
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Get output as JSON value if available
    pub fn as_json(&self) -> Option<&Value> {
        self.parsed.as_ref()
    }

    /// Get structured output if available
    pub fn as_structured(&self) -> Option<&StructuredOutput> {
        self.structured.as_ref()
    }
}

/// Context passed to tool execution
#[derive(Clone)]
pub struct ToolContext {
    pub filesystem_policy: FilesystemPolicy,
    pub network_policy: NetworkPolicy,
    pub allowed_tools: ToolSet,
    pub audit_log: Arc<Mutex<AuditLog>>,
}

/// Core tool trait
#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name
    fn name(&self) -> &str;

    /// Human-readable description
    fn description(&self) -> &str;

    /// JSON Schema for input arguments
    fn schema(&self) -> Value;

    /// Tool source
    fn source(&self) -> ToolSource;

    /// Execute the tool
    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError>;

    /// Stream output from a tool. Default impl wraps `call()` into a single final chunk.
    async fn call_streaming(
        &self,
        args: Value,
        ctx: ToolContext,
    ) -> Result<Vec<ToolChunk>, ToolError> {
        let output = self.call(args, ctx).await?;
        Ok(vec![ToolChunk {
            delta: output.raw,
            is_final: true,
        }])
    }
}
