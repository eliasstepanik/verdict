//! Skill registry for managing registered skills

use std::collections::HashMap;

use crate::skills::skill::Skill;

/// Registry for managing skills
#[derive(Debug, Clone)]
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    /// Create a new empty skill registry
    pub fn new() -> Self {
        Self {
            skills: HashMap::new(),
        }
    }

    /// Create a new skill registry with all built-in skills registered
    pub fn with_builtins() -> Self {
        let mut registry = Self::new();
        registry.register(crate::skills::builtin::rust_debugging());
        registry.register(crate::skills::builtin::code_review());
        registry.register(crate::skills::builtin::api_design());
        registry.register(crate::skills::builtin::test_writing());
        registry.register(crate::skills::builtin::refactoring());
        registry
    }

    /// Register a skill in the registry
    pub fn register(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    /// Get a registered skill by name
    pub fn get(&self, name: &str) -> Option<Skill> {
        self.skills.get(name).cloned()
    }

    /// List all registered skill names
    pub fn list(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}
