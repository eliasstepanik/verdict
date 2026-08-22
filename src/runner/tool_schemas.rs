// Tool schema building: constructs LLM-compatible tool schemas from the tool registry.
// Extracted from tool_use_loop.rs.

use crate::runner::PipelineRunner;

impl PipelineRunner {
    /// Build tool schemas from the tool registry, sanitizing names (dots → underscores).
    ///
    /// Converts each tool's metadata into an LLM-compatible schema and builds a mapping
    /// from sanitized names (for the LLM) back to the original tool registry names.
    ///
    /// Dots in tool names are replaced with underscores to ensure LLM compatibility;
    /// the resulting map allows reverse lookups to find the actual tool in the registry.
    pub(crate) fn build_tool_schemas(
        &self,
        tools: &[String],
    ) -> (Vec<crate::llm::ToolSchema>, std::collections::HashMap<String, String>) {
        let mut tool_schemas = Vec::new();
        let mut tool_name_map = std::collections::HashMap::new();

        for tool_name in tools {
            if let Some(tool) = self.tool_registry.get(tool_name) {
                let safe_name = tool_name.replace('.', "_");
                tool_name_map.insert(safe_name.clone(), tool_name.clone());
                tool_schemas.push(crate::llm::ToolSchema {
                    name: safe_name,
                    description: tool.description().to_string(),
                    parameters: tool.schema(),
                });
            }
        }

        (tool_schemas, tool_name_map)
    }
}
