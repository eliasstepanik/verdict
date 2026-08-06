//! Cancellation token support for pipeline interruption.
//!
//! Provides a cheap, cloneable token that signals cancellation to running operations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;

/// A token that signals cancellation to running operations.
///
/// Cheap to clone (Arc-backed). Safe to share across tasks.
/// When cancelled, all waiters are notified immediately.
#[derive(Clone, Debug)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl CancellationToken {
    /// Create a new token (not yet cancelled).
    pub fn new() -> Self {
        CancellationToken {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// Signal cancellation. Idempotent — multiple calls have no additional effect.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    /// Returns true if cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Await cancellation. Returns immediately if already cancelled.
    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }

    /// Create a child token that inherits parent cancellation.
    /// Currently returns a clone (shares the same flag).
    pub fn child_token(&self) -> Self {
        self.clone()
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cancellation_token_basics() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_cancellation_token_clone() {
        let token1 = CancellationToken::new();
        let token2 = token1.clone();
        token1.cancel();
        assert!(token2.is_cancelled());
    }

    #[tokio::test]
    async fn test_cancellation_await() {
        let token = CancellationToken::new();
        let token_clone = token.clone();
        let handle = tokio::spawn(async move {
            token_clone.cancelled().await;
            "finished"
        });
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        token.cancel();
        let result = handle.await.unwrap();
        assert_eq!(result, "finished");
    }

    #[tokio::test]
    async fn test_cancellation_immediate_if_already_cancelled() {
        let token = CancellationToken::new();
        token.cancel();
        // Should return immediately without deadlock
        token.cancelled().await;
    }
}
