use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;
use tracing::debug;

use crate::server::CommandDispatcher;
use crate::types::Envelope;
use crate::Result;

/// Dispatcher mode for testing different scenarios
#[derive(Debug, Clone, Copy)]
pub enum MockDispatcherMode {
    /// Push mode - supports auto-dispatch
    Push,
    /// Pull mode - leasing only
    Pull,
}

/// Mock command dispatcher for testing both push and pull scenarios
///
/// This dispatcher captures all dispatched envelopes instead of actually sending them,
/// allowing tests to verify that commands are properly dispatched and inspect the
/// envelope contents. Can be configured for push or pull mode testing.
#[derive(Debug)]
pub struct MockDispatcher {
    dispatched: Arc<Mutex<VecDeque<DispatchedEnvelope>>>,
    should_fail: Arc<Mutex<bool>>,
    mode: MockDispatcherMode,
}

/// A captured dispatched envelope with metadata
#[derive(Debug, Clone)]
pub struct DispatchedEnvelope {
    /// The envelope that was dispatched
    pub envelope: Envelope,
    /// Timestamp when the envelope was dispatched
    pub dispatched_at: chrono::DateTime<chrono::Utc>,
}

impl MockDispatcher {
    /// Create a new mock dispatcher in push mode (default)
    pub fn new() -> Self {
        Self::new_push()
    }

    /// Create a new mock dispatcher in push mode
    pub fn new_push() -> Self {
        Self {
            dispatched: Arc::new(Mutex::new(VecDeque::new())),
            should_fail: Arc::new(Mutex::new(false)),
            mode: MockDispatcherMode::Push,
        }
    }

    /// Create a new mock dispatcher in pull mode
    pub fn new_pull() -> Self {
        Self {
            dispatched: Arc::new(Mutex::new(VecDeque::new())),
            should_fail: Arc::new(Mutex::new(false)),
            mode: MockDispatcherMode::Pull,
        }
    }

    /// Get the dispatcher mode
    pub fn mode(&self) -> MockDispatcherMode {
        self.mode
    }

    /// Get the number of envelopes that have been dispatched
    pub async fn dispatch_count(&self) -> usize {
        let dispatched = self.dispatched.lock().await;
        dispatched.len()
    }

    /// Check if any envelopes have been dispatched
    pub async fn has_dispatched(&self) -> bool {
        self.dispatch_count().await > 0
    }

    /// Get all dispatched envelopes in order
    pub async fn get_dispatched(&self) -> Vec<DispatchedEnvelope> {
        let dispatched = self.dispatched.lock().await;
        dispatched.iter().cloned().collect()
    }

    /// Get the most recently dispatched envelope
    pub async fn get_latest(&self) -> Option<DispatchedEnvelope> {
        let dispatched = self.dispatched.lock().await;
        dispatched.back().cloned()
    }

    /// Get the first dispatched envelope
    pub async fn get_first(&self) -> Option<DispatchedEnvelope> {
        let dispatched = self.dispatched.lock().await;
        dispatched.front().cloned()
    }

    /// Pop the oldest dispatched envelope (FIFO)
    pub async fn pop_dispatched(&self) -> Option<DispatchedEnvelope> {
        let mut dispatched = self.dispatched.lock().await;
        dispatched.pop_front()
    }

    /// Clear all dispatched envelopes
    pub async fn clear(&self) {
        let mut dispatched = self.dispatched.lock().await;
        dispatched.clear();
    }

    /// Configure the dispatcher to fail on the next dispatch
    /// Useful for testing error handling
    pub async fn set_should_fail(&self, should_fail: bool) {
        let mut fail_flag = self.should_fail.lock().await;
        *fail_flag = should_fail;
    }

    /// Get dispatched envelopes for a specific command name
    pub async fn get_dispatched_for_command(&self, command: &str) -> Vec<DispatchedEnvelope> {
        let dispatched = self.dispatched.lock().await;
        dispatched
            .iter()
            .filter(|d| d.envelope.command == command)
            .cloned()
            .collect()
    }

    /// Get dispatched envelopes for a specific command ID
    pub async fn get_dispatched_for_command_id(&self, command_id: &str) -> Vec<DispatchedEnvelope> {
        let dispatched = self.dispatched.lock().await;
        dispatched
            .iter()
            .filter(|d| d.envelope.command_id == command_id)
            .cloned()
            .collect()
    }

    /// Wait for a specific number of dispatches to occur
    /// Returns true if the count is reached within the timeout
    pub async fn wait_for_dispatches(&self, count: usize, timeout: std::time::Duration) -> bool {
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            if self.dispatch_count().await >= count {
                return true;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        false
    }

    /// Wait for a dispatch of a specific command
    /// Returns the envelope if found within timeout
    pub async fn wait_for_command_dispatch(
        &self,
        command: &str,
        timeout: std::time::Duration,
    ) -> Option<DispatchedEnvelope> {
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            let dispatches = self.get_dispatched_for_command(command).await;
            if let Some(dispatch) = dispatches.first() {
                return Some(dispatch.clone());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        None
    }
}

impl Default for MockDispatcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CommandDispatcher for MockDispatcher {
    async fn dispatch(&self, envelope: &Envelope) -> Result<()> {
        // Check if we should simulate a failure
        {
            let mut should_fail = self.should_fail.lock().await;
            if *should_fail {
                *should_fail = false; // Reset after one failure
                return Err(alien_error::AlienError::new(
                    crate::error::ErrorData::TransportDispatchFailed {
                        message: "Mock dispatcher configured to fail".to_string(),
                        transport_type: Some("mock".to_string()),
                        target: Some(envelope.command_id.clone()),
                    },
                ));
            }
        }

        // Capture the envelope
        let dispatched_envelope = DispatchedEnvelope {
            envelope: envelope.clone(),
            dispatched_at: chrono::Utc::now(),
        };

        let mut dispatched = self.dispatched.lock().await;
        dispatched.push_back(dispatched_envelope.clone());

        debug!(
            command_id = %envelope.command_id,
            command = %envelope.command,
            "MockDispatcher: captured envelope dispatch"
        );

        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Helper trait for test assertions on MockDispatcher
#[async_trait]
pub trait MockDispatcherAssertions {
    /// Assert that exactly N envelopes were dispatched
    async fn assert_dispatch_count(&self, expected: usize);

    /// Assert that at least one envelope was dispatched
    async fn assert_has_dispatched(&self);

    /// Assert that no envelopes were dispatched
    async fn assert_not_dispatched(&self);

    /// Assert that an envelope was dispatched for a specific command
    async fn assert_dispatched_for_command(&self, command: &str);

    /// Assert that an envelope was dispatched for a specific command ID
    async fn assert_dispatched_for_command_id(&self, command_id: &str);
}

#[async_trait]
impl MockDispatcherAssertions for MockDispatcher {
    async fn assert_dispatch_count(&self, expected: usize) {
        let actual = self.dispatch_count().await;
        assert_eq!(
            actual, expected,
            "Expected {} dispatched envelopes, but found {}",
            expected, actual
        );
    }

    async fn assert_has_dispatched(&self) {
        assert!(
            self.has_dispatched().await,
            "Expected at least one envelope to be dispatched, but none were found"
        );
    }

    async fn assert_not_dispatched(&self) {
        assert!(
            !self.has_dispatched().await,
            "Expected no envelopes to be dispatched, but {} were found",
            self.dispatch_count().await
        );
    }

    async fn assert_dispatched_for_command(&self, command: &str) {
        let dispatches = self.get_dispatched_for_command(command).await;
        assert!(
            !dispatches.is_empty(),
            "Expected envelope to be dispatched for command '{}', but none were found",
            command
        );
    }

    async fn assert_dispatched_for_command_id(&self, command_id: &str) {
        let dispatches = self.get_dispatched_for_command_id(command_id).await;
        assert!(
            !dispatches.is_empty(),
            "Expected envelope to be dispatched for command ID '{}', but none were found",
            command_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BodySpec, ResponseHandling};
    use chrono::Utc;

    fn create_test_envelope(command_id: &str, command: &str) -> Envelope {
        Envelope::new(
            "test-agent".to_string(),
            command_id.to_string(),
            1,
            None,
            command.to_string(),
            BodySpec::inline(b"{}"),
            ResponseHandling {
                max_inline_bytes: 150000,
                submit_response_url: "https://arc.example.com/response".to_string(),
                storage_upload_request: alien_bindings::presigned::PresignedRequest::new_http(
                    "https://storage.example.com/upload".to_string(),
                    "PUT".to_string(),
                    std::collections::HashMap::new(),
                    alien_bindings::presigned::PresignedOperation::Put,
                    "test-path".to_string(),
                    Utc::now() + chrono::Duration::hours(1),
                ),
            },
        )
    }

    #[tokio::test]
    async fn test_basic_dispatch_capture() {
        let dispatcher = MockDispatcher::new();

        // Initially no dispatches
        assert!(!dispatcher.has_dispatched().await);
        assert_eq!(dispatcher.dispatch_count().await, 0);

        // Dispatch an envelope
        let envelope = create_test_envelope("cmd_123", "test-command");
        dispatcher.dispatch(&envelope).await.unwrap();

        // Should be captured
        assert!(dispatcher.has_dispatched().await);
        assert_eq!(dispatcher.dispatch_count().await, 1);

        let dispatched = dispatcher.get_dispatched().await;
        assert_eq!(dispatched.len(), 1);
        assert_eq!(dispatched[0].envelope.command_id, "cmd_123");
        assert_eq!(dispatched[0].envelope.command, "test-command");
    }

    #[tokio::test]
    async fn test_multiple_dispatches() {
        let dispatcher = MockDispatcher::new();

        // Dispatch multiple envelopes
        let envelope1 = create_test_envelope("cmd_1", "command-a");
        let envelope2 = create_test_envelope("cmd_2", "command-b");
        let envelope3 = create_test_envelope("cmd_3", "command-a");

        dispatcher.dispatch(&envelope1).await.unwrap();
        dispatcher.dispatch(&envelope2).await.unwrap();
        dispatcher.dispatch(&envelope3).await.unwrap();

        assert_eq!(dispatcher.dispatch_count().await, 3);

        // Test command filtering
        let command_a_dispatches = dispatcher.get_dispatched_for_command("command-a").await;
        assert_eq!(command_a_dispatches.len(), 2);

        let command_b_dispatches = dispatcher.get_dispatched_for_command("command-b").await;
        assert_eq!(command_b_dispatches.len(), 1);

        // Test command ID filtering
        let cmd2_dispatches = dispatcher.get_dispatched_for_command_id("cmd_2").await;
        assert_eq!(cmd2_dispatches.len(), 1);
        assert_eq!(cmd2_dispatches[0].envelope.command, "command-b");
    }

    #[tokio::test]
    async fn test_pop_and_clear() {
        let dispatcher = MockDispatcher::new();

        // Dispatch some envelopes
        let envelope1 = create_test_envelope("cmd_1", "command-1");
        let envelope2 = create_test_envelope("cmd_2", "command-2");

        dispatcher.dispatch(&envelope1).await.unwrap();
        dispatcher.dispatch(&envelope2).await.unwrap();

        assert_eq!(dispatcher.dispatch_count().await, 2);

        // Pop first (FIFO)
        let popped = dispatcher.pop_dispatched().await.unwrap();
        assert_eq!(popped.envelope.command_id, "cmd_1");
        assert_eq!(dispatcher.dispatch_count().await, 1);

        // Clear all
        dispatcher.clear().await;
        assert_eq!(dispatcher.dispatch_count().await, 0);
        assert!(!dispatcher.has_dispatched().await);
    }

    #[tokio::test]
    async fn test_failure_simulation() {
        let dispatcher = MockDispatcher::new();
        let envelope = create_test_envelope("cmd_fail", "fail-command");

        // Configure to fail
        dispatcher.set_should_fail(true).await;

        // Should fail
        let result = dispatcher.dispatch(&envelope).await;
        assert!(result.is_err());

        // Should not be captured since it failed
        assert!(!dispatcher.has_dispatched().await);

        // Next dispatch should succeed (failure flag is reset)
        let result = dispatcher.dispatch(&envelope).await;
        assert!(result.is_ok());
        assert!(dispatcher.has_dispatched().await);
    }

    #[tokio::test]
    async fn test_pull_mode() {
        let dispatcher = MockDispatcher::new_pull();
        let envelope = create_test_envelope("cmd_pull", "pull-command");

        let result = dispatcher.dispatch(&envelope).await;
        assert!(result.is_ok());
        assert!(dispatcher.has_dispatched().await);
    }

    #[tokio::test]
    async fn test_wait_operations() {
        let dispatcher = MockDispatcher::new();

        // Test wait for count with immediate dispatch
        let envelope = create_test_envelope("cmd_wait", "wait-command");
        dispatcher.dispatch(&envelope).await.unwrap();

        let result = dispatcher
            .wait_for_dispatches(1, std::time::Duration::from_millis(100))
            .await;
        assert!(result);

        // Test wait for command with timeout
        let result = dispatcher
            .wait_for_command_dispatch("wait-command", std::time::Duration::from_millis(100))
            .await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().envelope.command_id, "cmd_wait");

        // Test timeout case
        let result = dispatcher
            .wait_for_command_dispatch("nonexistent", std::time::Duration::from_millis(50))
            .await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_assertion_helpers() {
        let dispatcher = MockDispatcher::new();

        // Test not dispatched assertion
        dispatcher.assert_not_dispatched().await;

        // Dispatch an envelope
        let envelope = create_test_envelope("cmd_assert", "assert-command");
        dispatcher.dispatch(&envelope).await.unwrap();

        // Test various assertions
        dispatcher.assert_has_dispatched().await;
        dispatcher.assert_dispatch_count(1).await;
        dispatcher
            .assert_dispatched_for_command("assert-command")
            .await;
        dispatcher
            .assert_dispatched_for_command_id("cmd_assert")
            .await;
    }
}
