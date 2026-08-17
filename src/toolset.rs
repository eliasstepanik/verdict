use serde::{Deserialize, Serialize};

/// Scoped tool permissions for agent steps.
///
/// Determines which tools an agent or step can use. Supports various scoping strategies:
/// - Predefined levels: None, ReadOnly, ReadWrite, Full
/// - Explicit allowlists: Allow(Vec<String>)
/// - Explicit denylists: Deny(Vec<String>)
/// - Skill-based: FromSkill(String)
/// - Composition: Intersection, Union
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolSet {
    /// No tools allowed
    None,

    /// Read-only operations (e.g., file read, directory list)
    ReadOnly,

    /// Read and write operations
    ReadWrite,

    /// All tools allowed
    Full,

    /// Explicit allowlist of tool names
    Allow(Vec<String>),

    /// Explicit denylist of tool names (everything except these).
    /// Note: `Deny(vec![])` denies no tools and is functionally equivalent to `Full` — all tools are allowed.
    Deny(Vec<String>),

    /// Inherit tools from a named skill
    FromSkill(String),

    /// Intersection: tools allowed by both operands
    Intersection(Box<ToolSet>, Box<ToolSet>),

    /// Union: tools allowed by either operand
    Union(Box<ToolSet>, Box<ToolSet>),
}

impl ToolSet {
    /// Return the list of explicitly named tools, if this is an Allow set.
    /// Returns None for Full/ReadWrite/Deny/etc. (caller must enumerate registry instead).
    pub fn explicit_names(&self) -> Option<&[String]> {
        match self {
            ToolSet::Allow(names) => Some(names.as_slice()),
            ToolSet::Deny(names) => Some(names.as_slice()),
            _ => None,
        }
    }

    /// Check if a tool is allowed, resolving FromSkill via skill registry.
    /// This method requires access to the skill registry to properly resolve FromSkill variants.
    pub fn contains_with_skill_registry(
        &self,
        tool_name: &str,
        skill_registry: &crate::skills::registry::SkillRegistry,
    ) -> bool {
        match self {
            ToolSet::None => false,
            ToolSet::ReadOnly => {
                // Allow only read and search operations
                matches!(
                    tool_name,
                    "fs.read" | "fs.list" | "search.files" | "search.grep"
                )
            }
            ToolSet::ReadWrite => {
                // ReadOnly tools + fs.write + fs.delete
                matches!(
                    tool_name,
                    "fs.read"
                        | "fs.list"
                        | "search.files"
                        | "search.grep"
                        | "fs.write"
                        | "fs.delete"
                )
            }
            ToolSet::Full => true,
            ToolSet::Allow(tools) => tools.iter().any(|t| t == tool_name),
            ToolSet::Deny(tools) => !tools.iter().any(|t| t == tool_name),
            ToolSet::FromSkill(skill_name) => {
                // Resolve skill by name and check if tool is in its allowed_tools
                if let Some(skill) = skill_registry.get(skill_name) {
                    // Recursively check the skill's allowed_tools against this tool
                    skill.allowed_tools.contains_with_skill_registry(tool_name, skill_registry)
                } else {
                    // Unknown skill => deny access by default
                    false
                }
            }
            ToolSet::Intersection(left, right) => {
                left.contains_with_skill_registry(tool_name, skill_registry)
                    && right.contains_with_skill_registry(tool_name, skill_registry)
            }
            ToolSet::Union(left, right) => {
                left.contains_with_skill_registry(tool_name, skill_registry)
                    || right.contains_with_skill_registry(tool_name, skill_registry)
            }
        }
    }

    /// Check if a tool is allowed by this toolset.
    ///
    /// Checks tool membership across all ToolSet variants including
    /// intersection and union composition. Returns true if the tool is allowed.

    pub fn contains(&self, tool_name: &str) -> bool {
        match self {
            ToolSet::None => false,
            ToolSet::ReadOnly => {
                // Allow only read and search operations
                matches!(
                    tool_name,
                    "fs.read" | "fs.list" | "search.files" | "search.grep"
                )
            }
            ToolSet::ReadWrite => {
                // ReadOnly tools + fs.write + fs.delete
                matches!(
                    tool_name,
                    "fs.read"
                        | "fs.list"
                        | "search.files"
                        | "search.grep"
                        | "fs.write"
                        | "fs.delete"
                )
            }
            ToolSet::Full => true,
            ToolSet::Allow(tools) => tools.iter().any(|t| t == tool_name),
            ToolSet::Deny(tools) => !tools.iter().any(|t| t == tool_name),
            ToolSet::FromSkill(_skill_name) => {
                // Try to resolve the skill from the skill_registry if available.
                // Since ToolSet::contains() doesn't have direct access to registry,
                // this requires threading the registry through at the call site.
                // For now, return false by default (restrictive default).
                // The actual resolution happens in the call site (execution.rs)
                // which has access to the skill registry.
                false
            }
            ToolSet::Intersection(left, right) => {
                left.contains(tool_name) && right.contains(tool_name)
            }
            ToolSet::Union(left, right) => left.contains(tool_name) || right.contains(tool_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::registry::SkillRegistry;

    #[test]
    fn test_readonly_allows_read_tools() {
        let toolset = ToolSet::ReadOnly;
        assert!(toolset.contains("fs.read"));
        assert!(toolset.contains("fs.list"));
        assert!(toolset.contains("search.files"));
        assert!(toolset.contains("search.grep"));
    }

    #[test]
    fn test_readonly_denies_write_tools() {
        let toolset = ToolSet::ReadOnly;
        assert!(!toolset.contains("fs.write"));
        assert!(!toolset.contains("fs.delete"));
        assert!(!toolset.contains("shell.run"));
    }

    #[test]
    fn test_readwrite_allows_read_and_write_tools() {
        let toolset = ToolSet::ReadWrite;
        assert!(toolset.contains("fs.read"));
        assert!(toolset.contains("fs.list"));
        assert!(toolset.contains("search.files"));
        assert!(toolset.contains("search.grep"));
        assert!(toolset.contains("fs.write"));
        assert!(toolset.contains("fs.delete"));
    }

    #[test]
    fn test_readwrite_denies_shell_and_agent_tools() {
        let toolset = ToolSet::ReadWrite;
        assert!(!toolset.contains("shell.run"));
        assert!(!toolset.contains("shell.cargo_test"));
        assert!(!toolset.contains("call_agent"));
    }

    #[test]
    fn test_from_skill_with_valid_skill() {
        let registry = SkillRegistry::with_builtins();
        let toolset = ToolSet::FromSkill("rust_debugging".to_string());

        // rust_debugging allows: shell.cargo_check, shell.cargo_test, fs.read, fs.write
        assert!(toolset.contains_with_skill_registry("fs.read", &registry));
        assert!(toolset.contains_with_skill_registry("fs.write", &registry));
        assert!(toolset.contains_with_skill_registry("shell.cargo_check", &registry));
        assert!(toolset.contains_with_skill_registry("shell.cargo_test", &registry));

        // rust_debugging does not allow these
        assert!(!toolset.contains_with_skill_registry("shell.run", &registry));
        assert!(!toolset.contains_with_skill_registry("fs.delete", &registry));
        assert!(!toolset.contains_with_skill_registry("search.files", &registry));
    }

    #[test]
    fn test_from_skill_with_unknown_skill() {
        let registry = SkillRegistry::new();
        let toolset = ToolSet::FromSkill("nonexistent_skill".to_string());

        // Unknown skill denies everything
        assert!(!toolset.contains_with_skill_registry("fs.read", &registry));
        assert!(!toolset.contains_with_skill_registry("fs.write", &registry));
        assert!(!toolset.contains_with_skill_registry("shell.run", &registry));
    }

    #[test]
    fn test_from_skill_fallback_to_contains_returns_false() {
        let toolset = ToolSet::FromSkill("rust_debugging".to_string());
        // The basic contains() method returns false for FromSkill since it doesn't have registry access
        assert!(!toolset.contains("fs.read"));
    }

    #[test]
    fn test_allow_list() {
        let toolset = ToolSet::Allow(vec!["fs.read".to_string(), "shell.run".to_string()]);
        assert!(toolset.contains("fs.read"));
        assert!(toolset.contains("shell.run"));
        assert!(!toolset.contains("fs.write"));
        assert!(!toolset.contains("fs.list"));
    }

    #[test]
    fn test_deny_list() {
        let toolset = ToolSet::Deny(vec!["shell.run".to_string(), "fs.delete".to_string()]);
        assert!(toolset.contains("fs.read"));
        assert!(toolset.contains("fs.write"));
        assert!(!toolset.contains("shell.run"));
        assert!(!toolset.contains("fs.delete"));
    }

    #[test]
    fn test_intersection() {
        let left = ToolSet::ReadOnly;
        let right = ToolSet::Allow(vec!["fs.read".to_string(), "fs.write".to_string()]);
        let toolset = ToolSet::Intersection(Box::new(left), Box::new(right));

        // Intersection: tool must be in both
        assert!(toolset.contains("fs.read")); // in ReadOnly and in Allow list
        assert!(!toolset.contains("fs.write")); // in Allow but not in ReadOnly
        assert!(!toolset.contains("fs.list")); // in ReadOnly but not in Allow list
    }

    #[test]
    fn test_union() {
        let left = ToolSet::Allow(vec!["fs.read".to_string()]);
        let right = ToolSet::Allow(vec!["fs.write".to_string()]);
        let toolset = ToolSet::Union(Box::new(left), Box::new(right));

        // Union: tool can be in either
        assert!(toolset.contains("fs.read"));
        assert!(toolset.contains("fs.write"));
        assert!(!toolset.contains("fs.delete"));
    }

    #[test]
    fn test_full_allows_everything() {
        let toolset = ToolSet::Full;
        assert!(toolset.contains("fs.read"));
        assert!(toolset.contains("fs.write"));
        assert!(toolset.contains("shell.run"));
        assert!(toolset.contains("call_agent"));
    }

    #[test]
    fn test_none_denies_everything() {
        let toolset = ToolSet::None;
        assert!(!toolset.contains("fs.read"));
        assert!(!toolset.contains("fs.write"));
        assert!(!toolset.contains("shell.run"));
    }
}
