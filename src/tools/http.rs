//! HTTP tool support.

use crate::agent::NetworkPolicy;
use crate::tools::{Tool, ToolContext, ToolError, ToolOutput, ToolSource};
use async_trait::async_trait;
use serde_json::{json, Value};

/// An HTTP tool that makes HTTP requests.
pub struct HttpTool {
    pub name: String,
    pub description: String,
    pub base_url: String,
    pub allowed_paths: Vec<String>,
}

impl HttpTool {
    /// Create a new HTTP tool.
    pub fn new(name: impl Into<String>, description: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            base_url: base_url.into(),
            allowed_paths: vec![],
        }
    }

    /// Add allowed paths for this tool.
    pub fn with_allowed_paths(mut self, paths: Vec<String>) -> Self {
        self.allowed_paths = paths;
        self
    }
}

#[async_trait]
impl Tool for HttpTool {
    fn name(&self) -> &str {
        self.name.as_str()
    }

    fn description(&self) -> &str {
        self.description.as_str()
    }

    fn source(&self) -> ToolSource {
        ToolSource::Http {
            base_url: self.base_url.clone(),
        }
    }

    async fn call(&self, args: Value, ctx: ToolContext) -> Result<ToolOutput, ToolError> {
        // Check network policy
        if ctx.network_policy == NetworkPolicy::DenyAll {
            return Err(ToolError::ExecutionFailed {
                reason: "network policy denies all HTTP calls".into(),
            });
        }

        // Parse arguments
        let method = args
            .get("method")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing or invalid 'method'".into(),
            })?
            .to_uppercase();

        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::SchemaValidationFailed {
                reason: "missing or invalid 'path'".into(),
            })?;

        // Check allowed paths
        if !self.allowed_paths.is_empty() && !self.allowed_paths.contains(&path.to_string()) {
            return Err(ToolError::ExecutionFailed {
                reason: format!("path '{}' not in allowed list", path),
            });
        }

        // Parse optional body and headers
        let body = args.get("body").cloned();
        let headers = args.get("headers").and_then(|v| v.as_object()).cloned();

        // Build URL
        let url = format!("{}{}", self.base_url, path);

        // Build HTTP client and request
        let client = reqwest::Client::new();
        let mut request = match method.as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "PATCH" => client.patch(&url),
            "DELETE" => client.delete(&url),
            "HEAD" => client.head(&url),
            _ => {
                return Err(ToolError::SchemaValidationFailed {
                    reason: format!("unsupported HTTP method: {}", method),
                })
            }
        };

        // Add headers if provided
        if let Some(headers_obj) = headers {
            for (key, value) in headers_obj {
                if let Some(val_str) = value.as_str() {
                    request = request.header(&key, val_str);
                }
            }
        }

        // Add body if provided
        if let Some(body_val) = body {
            request = request.json(&body_val);
        }

        // Execute request
        let response = request.send().await.map_err(|e| {
            ToolError::ExecutionFailed {
                reason: format!("HTTP request failed: {}", e),
            }
        })?;

        let status_code = response.status().as_u16();
        let response_text = response.text().await.map_err(|e| {
            ToolError::ExecutionFailed {
                reason: format!("failed to read response body: {}", e),
            }
        })?;

        // Try to parse body as JSON, fall back to string
        let body_value = serde_json::from_str::<Value>(&response_text)
            .unwrap_or(Value::String(response_text));

        Ok(ToolOutput::json(json!({
            "status": status_code,
            "body": body_value
        })))
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "method": { "type": "string", "description": "HTTP method (GET, POST, PUT, PATCH, DELETE, HEAD)" },
                "path": { "type": "string", "description": "Request path" },
                "body": { "type": "object", "description": "Optional request body" },
                "headers": { "type": "object", "description": "Optional request headers" }
            },
            "required": ["method", "path"]
        })
    }
}
