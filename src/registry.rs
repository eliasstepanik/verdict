use std::collections::HashMap;
use std::sync::Arc;

use crate::agent::Agent;

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

/// Tool registry — Phase 2 stub
pub struct ToolRegistry;

/// Skill registry — Phase 5 stub
pub struct SkillRegistry;
