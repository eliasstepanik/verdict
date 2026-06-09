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

    /// Explicit denylist of tool names (everything except these)
    Deny(Vec<String>),

    /// Inherit tools from a named skill
    FromSkill(String),

    /// Intersection: tools allowed by both operands
    Intersection(Box<ToolSet>, Box<ToolSet>),

    /// Union: tools allowed by either operand
    Union(Box<ToolSet>, Box<ToolSet>),
}

impl ToolSet {
    /// Check if a tool is allowed by this toolset.
    ///
    /// In Phase 1, this is a basic stub that handles the common cases.
    /// Full enforcement including intersection/union resolution is Phase 2.
    pub fn contains(&self, tool_name: &str) -> bool {
        match self {
            ToolSet::None => false,
            ToolSet::ReadOnly => {
                // Stub: assume tool names starting with "fs.read", "dir.", "search." are read-only
                tool_name.starts_with("fs.read")
                    || tool_name.starts_with("dir.")
                    || tool_name.starts_with("search.")
            }
            ToolSet::ReadWrite => true,
            ToolSet::Full => true,
            ToolSet::Allow(tools) => tools.iter().any(|t| t == tool_name),
            ToolSet::Deny(tools) => !tools.iter().any(|t| t == tool_name),
            ToolSet::FromSkill(_) => {
                // Phase 1 stub: defer to Phase 5
                true
            }
            ToolSet::Intersection(left, right) => {
                left.contains(tool_name) && right.contains(tool_name)
            }
            ToolSet::Union(left, right) => {
                left.contains(tool_name) || right.contains(tool_name)
            }
        }
    }
}
