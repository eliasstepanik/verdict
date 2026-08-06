use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::Agent;
use crate::mcp::McpError;
use crate::tools::Tool;

/// Registry of available agents for delegation
#[derive(Debug)]
pub struct AgentRegistry {
    agents: HashMap<String, Arc<Agent>>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self {
            agents: HashMap::new(),
        }
    }

    pub fn register(&mut self, agent: Agent) {
        self.agents.insert(agent.name.clone(), Arc::new(agent));
    }

    pub fn get(&self, name: &str) -> Option<Arc<Agent>> {
        self.agents.get(name).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.agents.keys().cloned().collect()
    }

    pub fn list_agents(&self) -> Vec<Arc<Agent>> {
        self.agents.values().cloned().collect()
    }
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of available tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Tools that require approval before execution (A1)
    requires_approval: std::collections::HashSet<String>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("tools", &self.tools.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            requires_approval: std::collections::HashSet::new(),
        }
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        self.tools.insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn register_arc(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Register a tool that requires human approval before each call (A1)
    pub fn register_with_approval(&mut self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name.clone(), tool);
        self.requires_approval.insert(name);
    }

    /// Check if a tool requires approval (A1)
    pub fn requires_approval(&self, name: &str) -> bool {
        self.requires_approval.contains(name)
    }

    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    /// Register an MCP tool with server-namespaced name (mcp.{server}.{tool})
    pub fn register_mcp_tool(
        &mut self,
        server_name: &str,
        tool: crate::mcp::McpToolAdapter,
    ) -> Result<(), McpError> {
        // Create namespaced name: mcp.{server}.{tool}
        let namespaced_name = format!("mcp.{}.{}", server_name, tool.name());
        self.tools.insert(namespaced_name, Arc::new(tool));
        Ok(())
    }

    /// Create a registry with all built-in tools
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();

        // Register shell tools
        for tool in crate::tools::shell::shell_tools() {
            registry.register_arc(tool);
        }

        // Register filesystem tools
        for tool in crate::tools::filesystem::filesystem_tools() {
            registry.register_arc(tool);
        }

        // Register search tools
        for tool in crate::tools::search::search_tools() {
            registry.register_arc(tool);
        }

        registry
    }

    /// Clear the approval-required set (useful for testing)
    pub fn clear_approvals(&mut self) {
        self.requires_approval.clear();
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// Re-export SkillRegistry from skills module
pub use crate::skills::registry::SkillRegistry;
