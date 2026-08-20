// Miscellaneous step action handlers: UserInput, Sleep, SleepUntil, ForEach, Suspend, RemoteAgent.
// Extracted from execution.rs (Task 3).

use crate::action::StepOutput;
use crate::runner::PipelineRunner;

impl PipelineRunner {
    /// Handle UserInput action.
    /// Prompts the user for input and returns their response.
    pub(crate) async fn handle_user_input(
        &self,
        prompt: &str,
        _ctx: &mut crate::context::StepContext,
    ) -> Result<StepOutput, crate::action::StepError> {
        eprintln!("{} [y/N]: ", prompt);
        let stdin = std::io::stdin();
        let mut line = String::new();
        stdin
            .read_line(&mut line)
            .map_err(|e| crate::action::StepError::ActionFailed {
                reason: format!("Failed to read user input: {}", e),
            })?;

        Ok(StepOutput::new(line.trim().to_string()))
    }

    /// Handle Sleep action.
    /// Sleeps for the specified duration in milliseconds.
    pub(crate) async fn handle_sleep(
        &self,
        duration_ms: &u64,
        _ctx: &mut crate::context::StepContext,
    ) -> Result<StepOutput, crate::action::StepError> {
        tokio::time::sleep(std::time::Duration::from_millis(*duration_ms)).await;
        Ok(StepOutput::new(format!("Slept for {}ms", duration_ms)))
    }

    /// Handle SleepUntil action.
    /// Sleeps until the specified timestamp is reached.
    pub(crate) async fn handle_sleep_until(
        &self,
        timestamp: &chrono::DateTime<chrono::Utc>,
        _ctx: &mut crate::context::StepContext,
    ) -> Result<StepOutput, crate::action::StepError> {
        let now = chrono::Utc::now();
        if *timestamp > now {
            if let Ok(dur) = (*timestamp - now).to_std() {
                tokio::time::sleep(dur).await;
            }
        }
        Ok(StepOutput::new(format!("Slept until {}", timestamp)))
    }

    /// Handle ForEach action.
    /// Iterates over an array from a prior step's output, optionally in parallel.
    pub(crate) async fn handle_for_each(
        &self,
        input_array_key: &str,
        concurrency: &usize,
        collect_results: &bool,
        ctx: &mut crate::context::StepContext,
    ) -> Result<StepOutput, crate::action::StepError> {
        // Get items array from a prior step's output
        let items: Vec<serde_json::Value> = ctx
            .step_results
            .get(input_array_key)
            .and_then(|r| r.output.parsed.as_ref())
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut results: Vec<StepOutput> = Vec::new();

        if *concurrency <= 1 {
            // Sequential execution
            for item in &items {
                let output = StepOutput::with_parsed(item.to_string(), item.clone());
                results.push(output);
            }
        } else {
            // Bounded parallel execution
            use futures::stream::{self, StreamExt};
            let outputs: Vec<StepOutput> = stream::iter(items.iter())
                .map(|item| async move {
                    StepOutput::with_parsed(item.to_string(), item.clone())
                })
                .buffer_unordered(*concurrency)
                .collect()
                .await;
            results = outputs;
        }

        if *collect_results {
            let arr: Vec<serde_json::Value> = results
                .iter()
                .map(|r| {
                    r.parsed
                        .clone()
                        .unwrap_or(serde_json::Value::String(r.raw.clone()))
                })
                .collect();
            let json = serde_json::Value::Array(arr);
            Ok(StepOutput::with_parsed(json.to_string(), json))
        } else {
            Ok(results
                .into_iter()
                .last()
                .unwrap_or_else(|| StepOutput::new(String::new())))
        }
    }

    /// Handle Suspend action.
    /// Suspends execution and saves context state for later resumption.
    pub(crate) async fn handle_suspend(
        &self,
        reason: &str,
        resume_schema: &Option<serde_json::Value>,
        ctx: &mut crate::context::StepContext,
    ) -> Result<StepOutput, crate::action::StepError> {
        // Generate state token
        let state_token = uuid::Uuid::new_v4().to_string();

        // Save context via ContextStore if available
        if let Some(store) = &self.context_store {
            let _ = store.save(ctx).await;
        }

        // Return suspended state in output
        Ok(StepOutput::new(format!(
            "Suspended: {}. State token: {}. Resume schema: {}",
            reason,
            state_token,
            resume_schema
                .as_ref()
                .map(|s| s.to_string())
                .unwrap_or_else(|| "none".to_string())
        )))
    }

    /// Handle RemoteAgent action.
    /// Delegates to a remote agent via HTTP endpoint.
    pub(crate) async fn handle_remote_agent(
        &self,
        endpoint: &str,
        agent_name: &str,
        payload: &serde_json::Value,
        _ctx: &mut crate::context::StepContext,
    ) -> Result<StepOutput, crate::action::StepError> {
        use crate::agent::RemoteAgentClient;

        let client = RemoteAgentClient::new();
        let result = client
            .execute(endpoint, agent_name, payload.clone())
            .await
            .map_err(|e| crate::action::StepError::RemoteAgentFailed(e))?;

        Ok(StepOutput::new(result.to_string()))
    }
}
