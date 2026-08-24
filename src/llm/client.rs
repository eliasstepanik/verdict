//! LLM client for making completion requests.

use crate::llm::provider::{LlmChunk, LlmError, LlmProvider, LlmRequest, LlmResponse};
use futures::stream::Stream;
use std::fmt;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

/// Client for making LLM requests.
pub struct LlmClient {
    provider: Arc<dyn LlmProvider>,
    /// Optional rate limiter for controlling call frequency.
    /// When present, all calls to complete() will check the rate limit.
    rate_limiter: Option<Arc<Mutex<crate::budget::RateLimiter>>>,
}

impl fmt::Debug for LlmClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LlmClient")
            .field("provider", &self.provider.name())
            .finish()
    }
}

impl Clone for LlmClient {
    fn clone(&self) -> Self {
        Self {
            provider: Arc::clone(&self.provider),
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

impl LlmClient {
    /// Create a new LLM client with the given provider.
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            rate_limiter: None,
        }
    }

    /// Attach a rate limiter to this client. Enables fluent builder pattern.
    pub fn with_rate_limiter(
        mut self,
        rate_limiter: Arc<Mutex<crate::budget::RateLimiter>>,
    ) -> Self {
        self.rate_limiter = Some(rate_limiter);
        self
    }

    /// Make a completion request to the LLM.
    /// If a rate limiter is configured, it will be checked before calling the provider.
    pub async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        // Check rate limit at the choke point, before calling the provider
        if let Some(rate_limiter_mutex) = &self.rate_limiter {
            // Recover from poisoned lock (fail-closed: still enforce rate limiting).
            // If the lock was previously poisoned (a panic occurred while holding it),
            // we extract and use the inner RateLimiter anyway. This ensures that even
            // after a panic, we don't accidentally skip the rate-limit check. A poisoned
            // lock is a sign of past internal error, but the rate-limit accounting
            // should still be reasonably recoverable (call counts and timestamps, though
            // stale, are still valid for preventing runaway call rates).
            let mut rate_limiter = match rate_limiter_mutex.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };

            if let Err(budget_err) = rate_limiter.check_rate_limit() {
                return Err(LlmError::LocalRateLimit(budget_err.to_string()));
            }
        }

        self.provider.complete(req).await
    }

    /// Stream an LLM request, yielding chunks as they arrive.
    /// If a rate limiter is configured, it will be checked before streaming begins.
    pub fn stream(
        &self,
        request: LlmRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<LlmChunk, LlmError>> + Send>> {
        // Check rate limit before starting the stream
        if let Some(rate_limiter_mutex) = &self.rate_limiter {
            // Recover from poisoned lock (fail-closed: still enforce rate limiting).
            let mut rate_limiter = match rate_limiter_mutex.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };

            if let Err(budget_err) = rate_limiter.check_rate_limit() {
                // Return a stream that immediately yields a rate limit error
                use futures::stream;
                let err_msg = budget_err.to_string();
                return Box::pin(stream::once(async {
                    Err(LlmError::LocalRateLimit(err_msg))
                }));
            }
        }

        self.provider.stream(request)
    }

    /// Get the default model name for this client's provider.
    pub fn default_model(&self) -> &str {
        self.provider.default_model()
    }

    /// Create a client from environment variables.
    ///
    /// Reads:
    /// - `OPENAI_API_KEY` (required) — API key for authentication
    /// - `OPENAI_BASE_URL` (optional, default: "https://api.openai.com") — base URL
    /// - `OPENAI_MODEL` (optional, default: "gpt-4o") — default model name
    ///
    /// Returns `LlmError::NotConfigured` if `OPENAI_API_KEY` is absent or empty.
    pub fn from_env() -> Result<Self, LlmError> {
        Self::from_env_with_overrides(None, None, None)
    }

    /// Create a client from environment variables with optional overrides.
    ///
    /// This is primarily used for testing. The public API is `from_env()`.
    ///
    /// If an override is provided, it takes precedence over the environment variable.
    /// Pass `Some("")` (empty string) to simulate a missing environment variable.
    ///
    /// # Arguments
    /// - `api_key_override`: Optional API key. If provided, overrides `OPENAI_API_KEY`.
    /// - `base_url_override`: Optional base URL. If provided, overrides `OPENAI_BASE_URL`.
    /// - `model_override`: Optional model. If provided, overrides `OPENAI_MODEL`.
    pub fn from_env_with_overrides(
        api_key_override: Option<&str>,
        base_url_override: Option<&str>,
        model_override: Option<&str>,
    ) -> Result<Self, LlmError> {
        use crate::llm::provider::OpenAiCompatibleProvider;

        // Use override if provided, otherwise read from environment
        let api_key = if let Some(override_key) = api_key_override {
            if override_key.is_empty() {
                return Err(LlmError::NotConfigured);
            }
            override_key.to_string()
        } else {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|k| !k.is_empty())
                .ok_or(LlmError::NotConfigured)?
        };

        let base_url = base_url_override
            .filter(|u| !u.is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                std::env::var("OPENAI_BASE_URL")
                    .ok()
                    .filter(|u| !u.is_empty())
            })
            .unwrap_or_else(|| "https://api.openai.com".into());

        let model = model_override
            .map(|s| s.to_string())
            .or_else(|| std::env::var("OPENAI_MODEL").ok())
            .unwrap_or_else(|| "gpt-4o".into());

        let provider = OpenAiCompatibleProvider::new(base_url, api_key, model);
        Ok(Self::new(Arc::new(provider)))
    }
}
