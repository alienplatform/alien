use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use async_trait::async_trait;
use object_store::ObjectStore;
use reqwest::Url;
use tempfile::TempDir;

use alien_bindings::{
    providers::{kv::LocalKv, storage::LocalStorage},
    traits::{Kv, Storage},
};

use crate::{
    server::{create_axum_router, CommandDispatcher, CommandServer, InMemoryCommandRegistry},
    test_utils::{MockDispatcher, MockDispatcherMode},
    types::*,
    Result,
};
use alien_core::DeploymentModel;

/// Test server for command protocol integration testing
///
/// This provides a complete command server setup with local backends,
/// making it easy to write integration tests without external dependencies.
/// The server includes:
///
/// - Local disk-persisted KV store for command state
/// - Local filesystem storage for large payloads
/// - Mock dispatcher for testing push scenarios
/// - Real HTTP server for realistic testing
///
/// # Usage
///
/// ```rust
/// use alien_commands::test_utils::TestCommandServer;
///
/// #[tokio::test]
/// async fn test_command_flow() {
///     let server = TestCommandServer::new().await;
///     
///     // Create a command
///     let response = server.create_command(test_create_command()).await.unwrap();
///     
///     // Simulate deployment lease acquisition
///     let lease = server.acquire_lease("test-deployment").await.unwrap();
///     
///     // Simulate deployment response
///     server.submit_command_response(&lease.command_id, test_response()).await.unwrap();
///     
///     // Check final status
///     let status = server.get_command_status(&response.command_id).await.unwrap();
///     assert_eq!(status.state, CommandState::Succeeded);
/// }
/// ```
pub struct TestCommandServer {
    /// The underlying command server
    pub command_server: Arc<CommandServer>,
    /// HTTP server address
    pub server_addr: SocketAddr,
    /// Server shutdown handle
    pub shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Local KV store (for inspection/debugging)
    pub kv: Arc<LocalKv>,
    /// Local storage (for inspection/debugging)
    pub storage: Arc<LocalStorage>,
    /// Command dispatcher (for testing push scenarios)
    pub dispatcher: Arc<dyn CommandDispatcher>,
    /// Temporary directory (kept alive for the test duration)
    _temp_dir: TempDir,
}

impl TestCommandServer {
    /// Create a new test command server with default configuration
    pub async fn new() -> Self {
        Self::builder().build().await
    }

    /// Create a test command server builder for custom configuration
    pub fn builder() -> TestCommandServerBuilder {
        TestCommandServerBuilder::new()
    }

    /// Get the base URL of the test server
    pub fn base_url(&self) -> String {
        format!("http://{}", self.server_addr)
    }

    /// Get the command API base URL
    pub fn command_base_url(&self) -> String {
        let base = Url::parse(&self.base_url()).expect("Valid base URL");
        base.join("v1/").expect("Valid URL join").to_string()
    }

    // Convenience methods that delegate to the underlying command server

    /// Create a new command
    pub async fn create_command(
        &self,
        request: CreateCommandRequest,
    ) -> Result<CreateCommandResponse> {
        self.command_server.create_command(request).await
    }

    /// Mark upload as complete for storage-mode commands
    pub async fn upload_complete(
        &self,
        command_id: &str,
        upload_request: UploadCompleteRequest,
    ) -> Result<UploadCompleteResponse> {
        self.command_server
            .upload_complete(command_id, upload_request)
            .await
    }

    /// Get the status of a command
    pub async fn get_command_status(&self, command_id: &str) -> Result<CommandStatusResponse> {
        self.command_server.get_command_status(command_id).await
    }

    /// Submit a response from a deployment
    pub async fn submit_command_response(
        &self,
        command_id: &str,
        response: CommandResponse,
    ) -> Result<()> {
        self.command_server
            .submit_command_response(command_id, response)
            .await
    }

    /// Acquire leases for a polling deployment
    pub async fn acquire_lease(
        &self,
        deployment_id: &str,
        mut lease_request: LeaseRequest,
    ) -> Result<LeaseResponse> {
        lease_request.deployment_id = deployment_id.to_string();
        self.command_server
            .acquire_lease(deployment_id, &lease_request)
            .await
    }

    /// Acquire a single lease for a polling deployment
    pub async fn acquire_single_lease(&self, deployment_id: &str) -> Result<Option<LeaseInfo>> {
        let mut lease_request = LeaseRequest::default();
        lease_request.deployment_id = deployment_id.to_string();
        let response = self.acquire_lease(deployment_id, lease_request).await?;
        Ok(response.leases.into_iter().next())
    }

    /// Release a lease manually
    pub async fn release_lease(&self, command_id: &str, lease_id: &str) -> Result<()> {
        self.command_server
            .release_lease(command_id, lease_id)
            .await
    }

    // Server management methods

    /// Stop the HTTP server
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }

    // Testing utilities

    /// Wait for a command to reach a specific state
    pub async fn wait_for_state(
        &self,
        command_id: &str,
        expected_state: CommandState,
        timeout: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            if let Ok(status) = self.get_command_status(command_id).await {
                if status.state == expected_state {
                    return true;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        false
    }

    /// Wait for a command to complete (succeed or fail)
    pub async fn wait_for_completion(
        &self,
        command_id: &str,
        timeout: std::time::Duration,
    ) -> Result<CommandStatusResponse> {
        let start = std::time::Instant::now();

        while start.elapsed() < timeout {
            let status = self.get_command_status(command_id).await?;
            if status.state.is_terminal() {
                return Ok(status);
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        Err(alien_error::AlienError::new(crate::ErrorData::Other {
            message: format!("Command {} did not complete within timeout", command_id),
        }))
    }

    /// Reset all server state for clean test isolation
    pub async fn reset(&self) {
        let _ = self.kv.clear().await;
        // Note: LocalStorage doesn't have a clear() method like InMemoryStorage did
        // For testing, we rely on the temp directory being cleaned up

        // Only clear if we have a MockDispatcher
        if let Some(mock_dispatcher) = self.mock_dispatcher() {
            mock_dispatcher.clear().await;
        }
    }

    /// Get the number of commands currently in the KV store
    pub async fn command_count(&self) -> usize {
        // Count keys that start with "cmd:"
        let keys = self.kv.keys().await.unwrap_or_default();
        keys.iter()
            .filter(|k| k.starts_with("cmd:") && !k.contains(":lease"))
            .count()
    }

    /// Get the number of objects in storage
    pub async fn storage_object_count(&self) -> usize {
        // List all objects and count them
        let mut count = 0;
        let mut stream = self.storage.list(None);
        while let Some(_) = futures::stream::StreamExt::next(&mut stream).await {
            count += 1;
        }
        count
    }

    /// Get the mock dispatcher if this server is using one
    /// Returns None if using a different dispatcher type
    pub fn mock_dispatcher(&self) -> Option<&MockDispatcher> {
        self.dispatcher.as_any().downcast_ref::<MockDispatcher>()
    }

    /// Check if the server state is clean (no commands or storage objects)
    pub async fn is_clean(&self) -> bool {
        self.command_count().await == 0 && self.storage_object_count().await == 0
    }
}

/// Builder for creating test command servers with custom configuration
pub struct TestCommandServerBuilder {
    kv: Option<Arc<LocalKv>>,
    storage: Option<Arc<LocalStorage>>,
    dispatcher: Option<Arc<dyn CommandDispatcher>>,
}

impl TestCommandServerBuilder {
    fn new() -> Self {
        Self {
            kv: None,
            storage: None,
            dispatcher: None,
        }
    }

    /// Use a specific KV instance (useful for sharing state between tests)
    pub fn with_kv(mut self, kv: Arc<LocalKv>) -> Self {
        self.kv = Some(kv);
        self
    }

    /// Use a specific storage instance (useful for sharing state between tests)
    pub fn with_storage(mut self, storage: Arc<LocalStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Use a specific dispatcher instance (useful for testing push scenarios)
    pub fn with_dispatcher(mut self, dispatcher: Arc<dyn CommandDispatcher>) -> Self {
        self.dispatcher = Some(dispatcher);
        self
    }

    /// Configure the server for pull mode (deployments must lease commands)
    pub fn with_pull_mode(mut self) -> Self {
        self.dispatcher = Some(Arc::new(MockDispatcher::new_pull()) as Arc<dyn CommandDispatcher>);
        self
    }

    /// Build the test command server
    pub async fn build(self) -> TestCommandServer {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        let kv = if let Some(kv) = self.kv {
            kv
        } else {
            // Create a LocalKv using a separate KV directory within the temp dir
            let kv_path = temp_dir.path().join("kv.db");
            Arc::new(
                LocalKv::new(kv_path)
                    .await
                    .expect("Failed to create LocalKv for testing"),
            )
        };
        let storage = self.storage.unwrap_or_else(|| {
            // Create a LocalStorage using the temp directory
            Arc::new(
                LocalStorage::new_from_path(temp_dir.path().to_str().unwrap())
                    .expect("Failed to create LocalStorage for testing"),
            )
        });
        let dispatcher = self
            .dispatcher
            .unwrap_or_else(|| Arc::new(MockDispatcher::new()) as Arc<dyn CommandDispatcher>);

        // Determine deployment model based on dispatcher
        // If using MockDispatcher, use its mode; otherwise default to Pull
        let deployment_model = dispatcher
            .as_any()
            .downcast_ref::<MockDispatcher>()
            .map(|d| match d.mode() {
                MockDispatcherMode::Push => DeploymentModel::Push,
                MockDispatcherMode::Pull => DeploymentModel::Pull,
            })
            .unwrap_or(DeploymentModel::Pull);

        // Find a free port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("Failed to bind to port");
        let server_addr = listener.local_addr().expect("Failed to get local address");
        let base_url = format!("http://{}", server_addr);

        // Use the full API base URL so the server generates correct response URLs
        let command_base_url = {
            let base = Url::parse(&base_url).expect("Valid base URL");
            base.join("v1/").expect("Valid URL join").to_string()
        };

        let command_server = Arc::new(CommandServer::new(
            kv.clone() as Arc<dyn Kv>,
            storage.clone() as Arc<dyn Storage>,
            dispatcher.clone(),
            Arc::new(InMemoryCommandRegistry::with_deployment_model(
                deployment_model,
            )),
            command_base_url,
        ));

        let commands_router: Router<Arc<CommandServer>> = create_axum_router();
        let router = Router::new()
            .nest("/v1", commands_router)
            .with_state(command_server.clone());

        // Start the HTTP server
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    shutdown_rx.await.ok();
                })
                .await
                .expect("Server failed");
        });

        // Give the server a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        TestCommandServer {
            command_server,
            server_addr,
            shutdown_tx: Some(shutdown_tx),
            kv,
            storage,
            dispatcher,
            _temp_dir: temp_dir,
        }
    }
}

impl Drop for TestCommandServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

/// Helper trait for test assertions on TestCommandServer
#[async_trait]
pub trait TestCommandServerAssertions {
    /// Assert that a command is in the expected state
    async fn assert_command_state(&self, command_id: &str, expected_state: CommandState);

    /// Assert that a command completed successfully
    async fn assert_command_succeeded(&self, command_id: &str);

    /// Assert that a command failed
    async fn assert_command_failed(&self, command_id: &str);

    /// Assert that the server state is clean
    async fn assert_clean(&self);

    /// Assert that N commands exist in the KV store
    async fn assert_command_count(&self, expected: usize);

    /// Assert that N objects exist in storage
    async fn assert_storage_count(&self, expected: usize);
}

#[async_trait]
impl TestCommandServerAssertions for TestCommandServer {
    async fn assert_command_state(&self, command_id: &str, expected_state: CommandState) {
        let status = self
            .get_command_status(command_id)
            .await
            .unwrap_or_else(|_| panic!("Failed to get status for command {}", command_id));
        assert_eq!(
            status.state, expected_state,
            "Command {} expected to be in state {:?}, but was {:?}",
            command_id, expected_state, status.state
        );
    }

    async fn assert_command_succeeded(&self, command_id: &str) {
        self.assert_command_state(command_id, CommandState::Succeeded)
            .await;
    }

    async fn assert_command_failed(&self, command_id: &str) {
        self.assert_command_state(command_id, CommandState::Failed)
            .await;
    }

    async fn assert_clean(&self) {
        assert!(
            self.is_clean().await,
            "Expected server state to be clean, but found {} commands and {} storage objects",
            self.command_count().await,
            self.storage_object_count().await
        );
    }

    async fn assert_command_count(&self, expected: usize) {
        let actual = self.command_count().await;
        assert_eq!(
            actual, expected,
            "Expected {} commands in KV store, but found {}",
            expected, actual
        );
    }

    async fn assert_storage_count(&self, expected: usize) {
        let actual = self.storage_object_count().await;
        assert_eq!(
            actual, expected,
            "Expected {} objects in storage, but found {}",
            expected, actual
        );
    }
}
