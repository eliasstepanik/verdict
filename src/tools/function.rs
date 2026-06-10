//! Function-based tools

use async_trait::async_trait;
use serde_json::Value;
use std::pin::Pin;
use std::future::Future;
use std::sync::Arc;

use crate::tools::tool::{Tool, ToolOutput, ToolError, ToolSource, ToolContext};

/// A tool backed by a Rust async function
pub struct FunctionTool {
    pub name: String,
    pub description: String,
    pub schema: Value,
    pub func: Arc<
        dyn Fn(Value, ToolContext) -> Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send>>
            + Send
            + Sync,
    >,
}

impl FunctionTool {
    /// Create a new function-based tool
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        schema: Value,
        func: impl Fn(Value, ToolContext) -> Pin<Box<dyn Future<Output = Result<ToolOutput, ToolError>> + Send>>
            + Send
            + Sync
            + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            schema,
            func: std::sync::Arc::new(func),
        }
    }
}

#[async_trait]
impl Tool for FunctionTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn schema(&self) -> Value {
        self.schema.clone()
    }

    fn source(&self) -> ToolSource {
        ToolSource::LocalFunction
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        (self.func)(args, ctx).await
    }
}
