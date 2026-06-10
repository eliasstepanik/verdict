use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::Agent;
use crate::mcp::McpError;
use crate::tools::Tool;

/// Registry of available agents for delegation
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
}

impl Default for AgentRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry of available tools
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        self.tools
            .insert(tool.name().to_string(), Arc::new(tool));
    }

    pub fn register_arc(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.name().to_string(), tool);
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
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}

/// Skill registry — Phase 5 stub
pub struct SkillRegistry;
