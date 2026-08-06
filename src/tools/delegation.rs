//! Agent delegation tool
//!
//! Allows the LLM to invoke subagents via tool-calling in a ReAct loop.
//!
//! Since `PipelineRunner::run()` returns a non-`Send` future, and `Tool::call()` must return
//! a `Send` future (due to `#[async_trait]`), we use a channel-based request-response pattern:
//! The tool sends a delegation request to a dedicated handler task that owns the runner.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::registry::AgentRegistry;
use crate::tools::tool::{Tool, ToolContext, ToolError, ToolOutput, ToolSource};

/// Request to invoke a subagent via delegation
pub struct DelegationRequest {
    pub agent_name: String,
    pub task: Value,
    pub model_override: Option<String>,
    pub reply_tx: tokio::sync::oneshot::Sender<Result<String, String>>,
}

/// Invokes a subagent via the tool-calling interface.
///
/// This tool sends delegation requests over a channel to a dedicated handler task
/// that owns the runner. This avoids `Send` issues with the non-Send `runner.run()` future.
pub struct CallAgentTool {
    /// Reference to the agent registry (shared with all runners)
    agent_registry: Arc<AgentRegistry>,
    /// Channel sender for delegation requests (Send + Sync)
    deleg_tx: tokio::sync::mpsc::Sender<DelegationRequest>,
}

impl CallAgentTool {
    /// Create a new CallAgentTool with a delegation request channel.
    ///
    /// The `deleg_tx` is used to send requests to a dedicated handler task
    /// that owns the runner instance.
    pub fn new(
        agent_registry: Arc<AgentRegistry>,
        deleg_tx: tokio::sync::mpsc::Sender<DelegationRequest>,
    ) -> Self {
        Self {
            agent_registry,
            deleg_tx,
        }
    }
}

#[async_trait]
impl Tool for CallAgentTool {
    fn name(&self) -> &str {
        "call_agent"
    }

    fn description(&self) -> &str {
        "Invoke a subagent to handle a specialized task. Provide the agent name and task description."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "description": "Name of the agent to invoke (e.g., 'planner', 'coder', 'reviewer', 'debugger', 'reflector')"
                },
                "task": {
                    "type": "string",
                    "description": "Task description or input for the agent"
                },
                "model": {
                    "type": "string",
                    "description": "Model tier to use: 'claude-haiku-4-5-20251001' (simple/fast), 'claude-sonnet-4-6' (balanced), 'claude-opus-4-7' (complex/deep). If omitted, uses claude-sonnet-4-6."
                }
            },
            "required": ["agent", "task"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Builtin
    }

    async fn call(&self, args: Value, _ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        let agent_name = args
            .get("agent")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing or non-string 'agent' field".to_string(),
            })?
            .to_string();

        let task = args
            .get("task")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing or non-string 'task' field".to_string(),
            })?
            .to_string();

        let model_override = args
            .get("model")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        // Verify agent exists before sending request
        self.agent_registry
            .get(&agent_name)
            .ok_or_else(|| ToolError::ExecutionFailed {
                reason: format!(
                    "Agent '{}' not found in registry. Available agents: {}",
                    agent_name,
                    self.agent_registry.list().join(", ")
                ),
            })?;

        // Create oneshot channel for response
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

        // Send delegation request to handler task
        self.deleg_tx
            .send(DelegationRequest {
                agent_name: agent_name.clone(),
                task: Value::String(task),
                model_override,
                reply_tx,
            })
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("Failed to send delegation request: {}", e),
            })?;

        // Wait for response from handler task
        let output = reply_rx
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                reason: format!("Delegation handler closed: {}", e),
            })?
            .map_err(|err| ToolError::ExecutionFailed {
                reason: format!("Delegation failed: {}", err),
            })?;

        Ok(ToolOutput::text(output))
    }
}

/// Create a delegation tool with a channel for delegation requests.
///
/// The caller should spawn a delegation handler task that receives from `deleg_rx`
/// and owns the runner instance.
pub fn call_agent_tool(
    agent_registry: Arc<AgentRegistry>,
    deleg_tx: tokio::sync::mpsc::Sender<DelegationRequest>,
) -> Arc<CallAgentTool> {
    Arc::new(CallAgentTool::new(agent_registry, deleg_tx))
}
