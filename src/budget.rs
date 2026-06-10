//! Budget tracking and rate limiting — Phase 7

use std::time::Instant;
use thiserror::Error;

/// Errors that can occur during budget checking
#[derive(Debug, Error, Clone)]
pub enum BudgetError {
    #[error("Cost exceeded: spent ${spent}, limit ${max}")]
    CostExceeded { spent: f64, max: f64 },

    #[error("LLM call limit exceeded: {used} calls, max {max}")]
    LlmCallLimitExceeded { used: u32, max: u32 },

    #[error("Tool call limit exceeded: {used} calls, max {max}")]
    ToolCallLimitExceeded { used: u32, max: u32 },

    #[error("Runtime exceeded: {elapsed_secs}s, max {max_secs}s")]
    RuntimeExceeded {
        elapsed_secs: u64,
        max_secs: u64,
    },

    #[error("Rate limit exceeded: {calls_this_minute} calls, max {max_calls} per minute")]
    RateLimitExceeded {
        calls_this_minute: u32,
        max_calls: u32,
    },
}

/// Budget tracker for managing costs and resource usage
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    /// Maximum cost in USD, if set
    pub max_cost_usd: Option<f64>,

    /// Actual cost spent in USD
    pub spent_usd: f64,

    /// Maximum number of LLM calls, if set
    pub max_llm_calls: Option<u32>,

    /// Number of LLM calls made
    pub llm_calls: u32,

    /// Maximum number of tool calls, if set
    pub max_tool_calls: Option<u32>,

    /// Number of tool calls made
    pub tool_calls: u32,

    /// When tracking started
    pub start_time: Instant,

    /// Maximum runtime in seconds, if set
    pub max_runtime_seconds: Option<u64>,
}

impl BudgetTracker {
    /// Create a new budget tracker
    pub fn new() -> Self {
        Self {
            max_cost_usd: None,
            spent_usd: 0.0,
            max_llm_calls: None,
            llm_calls: 0,
            max_tool_calls: None,
            tool_calls: 0,
            start_time: Instant::now(),
            max_runtime_seconds: None,
        }
    }

    /// Set maximum cost in USD
    pub fn with_max_cost_usd(mut self, max_usd: f64) -> Self {
        self.max_cost_usd = Some(max_usd);
        self
    }

    /// Set maximum LLM calls
    pub fn with_max_llm_calls(mut self, max_calls: u32) -> Self {
        self.max_llm_calls = Some(max_calls);
        self
    }

    /// Set maximum tool calls
    pub fn with_max_tool_calls(mut self, max_calls: u32) -> Self {
        self.max_tool_calls = Some(max_calls);
        self
    }

    /// Set maximum runtime in seconds
    pub fn with_max_runtime_seconds(mut self, max_seconds: u64) -> Self {
        self.max_runtime_seconds = Some(max_seconds);
        self
    }

    /// Record an LLM call with cost
    pub fn record_llm_call(&mut self, cost_usd: f64) {
        self.llm_calls += 1;
        self.spent_usd += cost_usd;
    }

    /// Record a tool call
    pub fn record_tool_call(&mut self) {
        self.tool_calls += 1;
    }

    /// Check if all limits are satisfied
    pub fn check_limits(&self) -> Result<(), BudgetError> {
        // Check cost
        if let Some(max) = self.max_cost_usd {
            if self.spent_usd > max {
                return Err(BudgetError::CostExceeded {
                    spent: self.spent_usd,
                    max,
                });
            }
        }

        // Check LLM calls
        if let Some(max) = self.max_llm_calls {
            if self.llm_calls > max {
                return Err(BudgetError::LlmCallLimitExceeded {
                    used: self.llm_calls,
                    max,
                });
            }
        }

        // Check tool calls
        if let Some(max) = self.max_tool_calls {
            if self.tool_calls > max {
                return Err(BudgetError::ToolCallLimitExceeded {
                    used: self.tool_calls,
                    max,
                });
            }
        }

        // Check runtime
        if let Some(max_secs) = self.max_runtime_seconds {
            let elapsed = self.start_time.elapsed().as_secs();
            if elapsed > max_secs {
                return Err(BudgetError::RuntimeExceeded {
                    elapsed_secs: elapsed,
                    max_secs,
                });
            }
        }

        Ok(())
    }

    /// Get remaining budget in USD
    pub fn remaining_usd(&self) -> Option<f64> {
        self.max_cost_usd.map(|max| (max - self.spent_usd).max(0.0))
    }

    /// Get remaining LLM calls
    pub fn remaining_llm_calls(&self) -> Option<u32> {
        self.max_llm_calls.map(|max| (max as i32 - self.llm_calls as i32).max(0) as u32)
    }

    /// Get remaining tool calls
    pub fn remaining_tool_calls(&self) -> Option<u32> {
        self.max_tool_calls.map(|max| (max as i32 - self.tool_calls as i32).max(0) as u32)
    }

    /// Get elapsed time in seconds
    pub fn elapsed_seconds(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    /// Get remaining time in seconds
    pub fn remaining_seconds(&self) -> Option<u64> {
        self.max_runtime_seconds.and_then(|max| {
            let elapsed = self.elapsed_seconds();
            if elapsed > max {
                Some(0)
            } else {
                Some(max - elapsed)
            }
        })
    }
}

impl Default for BudgetTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Rate limiter for controlling call frequency
#[derive(Debug, Clone)]
pub struct RateLimiter {
    /// Maximum calls per minute, if set
    pub max_calls_per_minute: Option<u32>,

    /// Number of calls made in current window
    pub calls_this_minute: u32,

    /// When the current window started
    pub window_start: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new() -> Self {
        Self {
            max_calls_per_minute: None,
            calls_this_minute: 0,
            window_start: Instant::now(),
        }
    }

    /// Set maximum calls per minute
    pub fn with_max_calls_per_minute(mut self, max_calls: u32) -> Self {
        self.max_calls_per_minute = Some(max_calls);
        self
    }

    /// Check if rate limit is satisfied
    pub fn check_rate_limit(&mut self) -> Result<(), BudgetError> {
        // Reset window if minute has passed
        if self.window_start.elapsed().as_secs() >= 60 {
            self.calls_this_minute = 0;
            self.window_start = Instant::now();
        }

        // Check limit
        if let Some(max) = self.max_calls_per_minute {
            if self.calls_this_minute >= max {
                return Err(BudgetError::RateLimitExceeded {
                    calls_this_minute: self.calls_this_minute,
                    max_calls: max,
                });
            }
        }

        Ok(())
    }

    /// Record a call (after checking rate limit)
    pub fn record_call(&mut self) {
        self.calls_this_minute += 1;
    }

    /// Get remaining calls in current window
    pub fn remaining_calls_this_minute(&self) -> Option<u32> {
        self.max_calls_per_minute.map(|max| {
            (max as i32 - self.calls_this_minute as i32).max(0) as u32
        })
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_tracker_creation() {
        let tracker = BudgetTracker::new();
        assert_eq!(tracker.spent_usd, 0.0);
        assert_eq!(tracker.llm_calls, 0);
        assert_eq!(tracker.tool_calls, 0);
    }

    #[test]
    fn test_budget_tracker_with_limits() {
        let tracker = BudgetTracker::new()
            .with_max_cost_usd(10.0)
            .with_max_llm_calls(5)
            .with_max_tool_calls(10);

        assert_eq!(tracker.max_cost_usd, Some(10.0));
        assert_eq!(tracker.max_llm_calls, Some(5));
        assert_eq!(tracker.max_tool_calls, Some(10));
    }

    #[test]
    fn test_budget_tracker_record_llm_call() {
        let mut tracker = BudgetTracker::new();
        tracker.record_llm_call(0.5);
        assert_eq!(tracker.llm_calls, 1);
        assert_eq!(tracker.spent_usd, 0.5);

        tracker.record_llm_call(0.3);
        assert_eq!(tracker.llm_calls, 2);
        assert_eq!(tracker.spent_usd, 0.8);
    }

    #[test]
    fn test_budget_tracker_record_tool_call() {
        let mut tracker = BudgetTracker::new();
        tracker.record_tool_call();
        assert_eq!(tracker.tool_calls, 1);

        tracker.record_tool_call();
        assert_eq!(tracker.tool_calls, 2);
    }

    #[test]
    fn test_budget_tracker_check_limits_cost_exceeded() {
        let mut tracker = BudgetTracker::new().with_max_cost_usd(1.0);
        tracker.spent_usd = 1.5;

        let result = tracker.check_limits();
        assert!(result.is_err());
        match result {
            Err(BudgetError::CostExceeded { spent, max }) => {
                assert_eq!(spent, 1.5);
                assert_eq!(max, 1.0);
            }
            _ => panic!("Expected CostExceeded error"),
        }
    }

    #[test]
    fn test_budget_tracker_check_limits_llm_calls_exceeded() {
        let mut tracker = BudgetTracker::new().with_max_llm_calls(3);
        tracker.llm_calls = 5;

        let result = tracker.check_limits();
        assert!(result.is_err());
        match result {
            Err(BudgetError::LlmCallLimitExceeded { used, max }) => {
                assert_eq!(used, 5);
                assert_eq!(max, 3);
            }
            _ => panic!("Expected LlmCallLimitExceeded error"),
        }
    }

    #[test]
    fn test_budget_tracker_check_limits_tool_calls_exceeded() {
        let mut tracker = BudgetTracker::new().with_max_tool_calls(5);
        tracker.tool_calls = 10;

        let result = tracker.check_limits();
        assert!(result.is_err());
        match result {
            Err(BudgetError::ToolCallLimitExceeded { used, max }) => {
                assert_eq!(used, 10);
                assert_eq!(max, 5);
            }
            _ => panic!("Expected ToolCallLimitExceeded error"),
        }
    }

    #[test]
    fn test_budget_tracker_check_limits_all_pass() {
        let tracker = BudgetTracker::new()
            .with_max_cost_usd(10.0)
            .with_max_llm_calls(5)
            .with_max_tool_calls(10);

        assert!(tracker.check_limits().is_ok());
    }

    #[test]
    fn test_budget_tracker_remaining_usd() {
        let tracker = BudgetTracker::new()
            .with_max_cost_usd(10.0);
        let mut tracker = tracker;
        tracker.spent_usd = 3.0;

        assert_eq!(tracker.remaining_usd(), Some(7.0));
    }

    #[test]
    fn test_rate_limiter_creation() {
        let limiter = RateLimiter::new();
        assert_eq!(limiter.calls_this_minute, 0);
        assert!(limiter.max_calls_per_minute.is_none());
    }

    #[test]
    fn test_rate_limiter_with_max_calls() {
        let limiter = RateLimiter::new().with_max_calls_per_minute(5);
        assert_eq!(limiter.max_calls_per_minute, Some(5));
    }

    #[test]
    fn test_rate_limiter_check_passes() {
        let mut limiter = RateLimiter::new().with_max_calls_per_minute(5);
        assert!(limiter.check_rate_limit().is_ok());
    }

    #[test]
    fn test_rate_limiter_exceeded() {
        let mut limiter = RateLimiter::new().with_max_calls_per_minute(2);
        limiter.calls_this_minute = 2;

        let result = limiter.check_rate_limit();
        assert!(result.is_err());
        match result {
            Err(BudgetError::RateLimitExceeded {
                calls_this_minute,
                max_calls,
            }) => {
                assert_eq!(calls_this_minute, 2);
                assert_eq!(max_calls, 2);
            }
            _ => panic!("Expected RateLimitExceeded error"),
        }
    }

    #[test]
    fn test_rate_limiter_record_call() {
        let mut limiter = RateLimiter::new();
        limiter.record_call();
        assert_eq!(limiter.calls_this_minute, 1);

        limiter.record_call();
        assert_eq!(limiter.calls_this_minute, 2);
    }
}
