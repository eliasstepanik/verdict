//! Tool registry and implementations

pub mod delegation;
pub mod external_command;
pub mod filesystem;
pub mod function;
pub mod http;
pub mod search;
pub mod shell;
pub mod tool;

pub use delegation::{call_agent_tool, CallAgentTool, DelegationRequest};
pub use external_command::ExternalCommandTool;
pub use filesystem::{DeleteFileTool, ListDirTool, ReadFileTool, WriteFileTool};
pub use function::FunctionTool;
pub use http::HttpTool;
pub use search::{GrepTool, SearchFilesTool};
pub use shell::{CargoCheckTool, CargoFmtTool, CargoTestTool, RunCommandTool};
pub use tool::{
    DiagnosticEntry, DiagnosticSeverity, StructuredOutput, Tool, ToolChunk, ToolContext, ToolError,
    ToolOutput, ToolSource,
};
