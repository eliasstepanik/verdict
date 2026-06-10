//! MCP (Model Context Protocol) server support — Phase 3
//! Provides client connection, tool discovery, and tool adaptation for MCP servers.

pub mod client;
pub mod server;
pub mod tool_adapter;

pub use client::{McpClient, McpError, DiscoveredTool};
pub use server::McpServerConfig;
pub use tool_adapter::McpToolAdapter;
