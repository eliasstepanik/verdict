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
