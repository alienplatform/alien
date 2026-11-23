use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use axum::Router;
use object_store::ObjectStore;
use reqwest::Url;
use tempfile::TempDir;

use alien_bindings::{
    providers::{kv::LocalKv, storage::LocalStorage},
    traits::{Binding, Kv, KvEntry, PutOptions, ScanResult, Storage},
    ErrorData,
};

use crate::{
    server::{create_axum_router, CommandDispatcher, CommandServer, InMemoryCommandRegistry},
    test_utils::{MockDispatcher, MockDispatcherMode},
    types::*,
    Result,
};

/// A [`Kv`] decorator that can be armed to fail the pending-index scan performed
/// during response cleanup, simulating a backend error or process failure that
/// strikes *after* the response blob has been stored.
///
/// It exists to prove that `submit_command_response` commits the terminal
/// registry state (the source of truth) *before* it touches the lease and
/// pending index: with the fault armed, the cleanup scan fails, yet the command
/// must still be observable as terminal with its stored response. Under the old
/// ordering (cleanup first, state last) the same fault left the command stranded
/// as `Dispatched` with an invisible response.
#[derive(Debug)]
pub struct FaultInjectingKv {
    inner: Arc<LocalKv>,
    fail_pending_scan: AtomicBool,
    fail_next_response_store: AtomicBool,
    idempotency_get_barrier: Mutex<Option<Arc<tokio::sync::Barrier>>>,
    fail_wake_key_writes: AtomicBool,
    fail_wake_key_reads: AtomicBool,
    hang_wake_key_reads: AtomicBool,
    wake_key_reads: AtomicUsize,
    pending_scans: AtomicUsize,
    /// When set, a plain pending-index scan does not see writes younger than
    /// this (a store whose scans lag behind acknowledged writes); a consistent
    /// scan always does.
    scan_lag: Mutex<Option<std::time::Duration>>,
    pending_writes: Mutex<std::collections::HashMap<String, std::time::Instant>>,
    #[allow(clippy::type_complexity)]
    after_pending_scan: HookSlot,
}

struct HookSlot(Mutex<Option<AfterScanHook>>);

impl std::fmt::Debug for HookSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HookSlot")
    }
}

/// Runs once, after a pending-index scan has returned its answer.
pub type AfterScanHook =
    Box<dyn FnOnce() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send>;

impl FaultInjectingKv {
    fn new(inner: Arc<LocalKv>) -> Self {
        Self {
            inner,
            fail_pending_scan: AtomicBool::new(false),
            fail_next_response_store: AtomicBool::new(false),
            idempotency_get_barrier: Mutex::new(None),
            fail_wake_key_writes: AtomicBool::new(false),
            fail_wake_key_reads: AtomicBool::new(false),
            hang_wake_key_reads: AtomicBool::new(false),
            wake_key_reads: AtomicUsize::new(0),
            scan_lag: Mutex::new(None),
            pending_writes: Mutex::new(Default::default()),
            pending_scans: AtomicUsize::new(0),
            after_pending_scan: HookSlot(Mutex::new(None)),
        }
    }

    /// Number of pending-index scans made so far (leasing and cleanup).
    pub fn pending_scans(&self) -> usize {
        self.pending_scans.load(Ordering::SeqCst)
    }

    /// Number of wake key reads made so far.
    pub fn wake_key_reads(&self) -> usize {
        self.wake_key_reads.load(Ordering::SeqCst)
    }

    /// Arm the fault: reads of a target's lease wake key fail.
    pub fn arm_wake_key_read_failure(&self) {
        self.fail_wake_key_reads.store(true, Ordering::SeqCst);
    }

    /// Arm the fault: reads of a target's lease wake key never return, like a
    /// store that stopped answering.
    pub fn arm_wake_key_read_hang(&self) {
        self.hang_wake_key_reads.store(true, Ordering::SeqCst);
    }

    /// Make plain pending-index scans lag: they miss entries written less than
    /// `lag` ago. Consistent scans are unaffected.
    pub fn arm_lagging_pending_scans(&self, lag: std::time::Duration) {
        *self.scan_lag.lock().expect("scan lag lock") = Some(lag);
    }

    /// Arm the fault: writes of a target's lease wake key fail.
    pub fn arm_wake_key_write_failure(&self) {
        self.fail_wake_key_writes.store(true, Ordering::SeqCst);
    }

    /// Run `hook` once, right after the next pending-index scan returned its
    /// answer: the scan's result is already decided when the hook acts, so
    /// whatever the hook queues lands between that scan and whatever follows.
    pub fn arm_after_next_pending_scan(&self, hook: AfterScanHook) {
        *self
            .after_pending_scan
            .0
            .lock()
            .expect("after-scan hook lock should not be poisoned") = Some(hook);
    }

    /// Arm the fault: subsequent `scan_prefix` calls over a `target:…` (pending
    /// index) prefix fail. Acquire leases *before* arming — lease acquisition
    /// scans the same prefix.
    pub fn arm_pending_scan_failure(&self) {
        self.fail_pending_scan.store(true, Ordering::SeqCst);
    }

    /// Arm the fault: the next write of a command's stored response fails, like
    /// a transient server error after the lease was claimed.
    pub fn arm_response_store_failure(&self) {
        self.fail_next_response_store.store(true, Ordering::SeqCst);
    }

    /// Hold the first `parties` absent idempotency reads until all concurrent
    /// creators have passed the pre-create check.
    pub fn arm_idempotency_get_barrier(&self, parties: usize) {
        *self
            .idempotency_get_barrier
            .lock()
            .expect("idempotency barrier lock should not be poisoned") =
            Some(Arc::new(tokio::sync::Barrier::new(parties)));
    }
}

impl Binding for FaultInjectingKv {}

#[async_trait]
impl Kv for FaultInjectingKv {
    async fn get(&self, key: &str) -> alien_bindings::Result<Option<KvEntry>> {
        if key.ends_with(":wake") {
            self.wake_key_reads.fetch_add(1, Ordering::SeqCst);
            if self.hang_wake_key_reads.load(Ordering::SeqCst) {
                std::future::pending::<()>().await;
            }
            if self.fail_wake_key_reads.load(Ordering::SeqCst) {
                return Err(alien_error::AlienError::new(ErrorData::KvOperationFailed {
                    operation: "get".to_string(),
                    key: key.to_string(),
                    reason: "injected fault: wake key read failure".to_string(),
                }));
            }
        }
        let value = self.inner.get(key).await?;
        let barrier = if key.starts_with("idem:") && value.is_none() {
            self.idempotency_get_barrier
                .lock()
                .expect("idempotency barrier lock should not be poisoned")
                .clone()
        } else {
            None
        };
        if let Some(barrier) = barrier {
            barrier.wait().await;
        }
        Ok(value)
    }

    async fn put(
        &self,
        key: &str,
        value: Vec<u8>,
        options: Option<PutOptions>,
    ) -> alien_bindings::Result<bool> {
        if key.ends_with(":wake") && self.fail_wake_key_writes.load(Ordering::SeqCst) {
            return Err(alien_error::AlienError::new(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.to_string(),
                reason: "injected fault: wake key write failure".to_string(),
            }));
        }
        if key.ends_with(":response") && self.fail_next_response_store.swap(false, Ordering::SeqCst)
        {
            return Err(alien_error::AlienError::new(ErrorData::KvOperationFailed {
                operation: "put".to_string(),
                key: key.to_string(),
                reason: "injected fault: response store failure".to_string(),
            }));
        }
        if key.contains(":pending:") {
            self.pending_writes
                .lock()
                .expect("pending writes lock")
                .insert(key.to_string(), std::time::Instant::now());
        }
        self.inner.put(key, value, options).await
    }

    async fn delete(&self, key: &str, if_version: Option<&str>) -> alien_bindings::Result<bool> {
        self.inner.delete(key, if_version).await
    }

    async fn exists(&self, key: &str) -> alien_bindings::Result<bool> {
        self.inner.exists(key).await
    }

    async fn scan_prefix(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> alien_bindings::Result<ScanResult> {
        self.scan(prefix, limit, cursor, false).await
    }

    async fn scan_prefix_consistent(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
    ) -> alien_bindings::Result<ScanResult> {
        self.scan(prefix, limit, cursor, true).await
    }
}

impl FaultInjectingKv {
    async fn scan(
        &self,
        prefix: &str,
        limit: Option<usize>,
        cursor: Option<String>,
        consistent: bool,
    ) -> alien_bindings::Result<ScanResult> {
        if self.fail_pending_scan.load(Ordering::SeqCst) && prefix.starts_with("target:") {
            return Err(alien_error::AlienError::new(ErrorData::KvOperationFailed {
                operation: "scan_prefix".to_string(),
                key: prefix.to_string(),
                reason: "injected fault: pending-index scan failure".to_string(),
            }));
        }
        let mut result = self.inner.scan_prefix(prefix, limit, cursor).await;
        if prefix.contains(":pending:") {
            let lag = *self.scan_lag.lock().expect("scan lag lock");
            if let (Some(lag), false, Ok(found)) = (lag, consistent, result.as_mut()) {
                let writes = self.pending_writes.lock().expect("pending writes lock");
                found.items.retain(|entry| {
                    writes
                        .get(&entry.key)
                        .is_none_or(|written| written.elapsed() >= lag)
                });
            }
            self.pending_scans.fetch_add(1, Ordering::SeqCst);
            let hook = self
                .after_pending_scan
                .0
                .lock()
                .expect("after-scan hook lock should not be poisoned")
                .take();
            if let Some(hook) = hook {
                hook().await;
            }
        }
        result
    }
}

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
    /// In-memory command registry (register extra targets for multi-target tests)
    pub registry: Arc<InMemoryCommandRegistry>,
    /// The auto-registered default command target
    /// ("test-worker" in push mode, "test-daemon" in pull mode)
    pub default_target: CommandTarget,
    /// The fault-injecting KV decorator wrapping `kv`, present only when the
    /// server was built with [`TestCommandServerBuilder::with_fault_injection`].
    /// Arm it to exercise mid-cleanup failures in `submit_command_response`.
    pub fault_kv: Option<Arc<FaultInjectingKv>>,
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

    /// Acquire a single lease for a polling deployment, as the server's
    /// auto-registered default target.
    pub async fn acquire_single_lease(&self, deployment_id: &str) -> Result<Option<LeaseInfo>> {
        let lease_request = LeaseRequest {
            deployment_id: deployment_id.to_string(),
            target: self.default_target.clone(),
            max_leases: 1,
            lease_seconds: 60,
            wait_seconds: 0,
        };
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
    fault_injection: bool,
    max_open_lease_waits: Option<usize>,
    max_open_lease_waits_per_target: Option<usize>,
    max_open_lease_waits_per_deployment: Option<usize>,
    lease_wait_probe_interval: Option<std::time::Duration>,
}

impl TestCommandServerBuilder {
    fn new() -> Self {
        Self {
            kv: None,
            storage: None,
            dispatcher: None,
            fault_injection: false,
            max_open_lease_waits: None,
            max_open_lease_waits_per_target: None,
            max_open_lease_waits_per_deployment: None,
            lease_wait_probe_interval: None,
        }
    }

    /// Limit the lease requests the server holds open while waiting.
    pub fn with_max_open_lease_waits(mut self, max: usize) -> Self {
        self.max_open_lease_waits = Some(max);
        self
    }

    /// Limit the lease requests the server holds open for one target.
    pub fn with_max_open_lease_waits_per_target(mut self, max: usize) -> Self {
        self.max_open_lease_waits_per_target = Some(max);
        self
    }

    /// Limit the lease requests the server holds open for one deployment.
    pub fn with_max_open_lease_waits_per_deployment(mut self, max: usize) -> Self {
        self.max_open_lease_waits_per_deployment = Some(max);
        self
    }

    /// How often the waiting lease requests of a target read its wake key.
    pub fn with_lease_wait_probe_interval(mut self, interval: std::time::Duration) -> Self {
        self.lease_wait_probe_interval = Some(interval);
        self
    }

    /// Wrap the KV store in a [`FaultInjectingKv`] so tests can simulate a
    /// backend/process failure partway through response cleanup.
    pub fn with_fault_injection(mut self) -> Self {
        self.fault_injection = true;
        self
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

        // Determine worker delivery mode based on dispatcher: if using a
        // MockDispatcher, its mode stands in for the production derivation
        // (platform push path + stack deployment model); otherwise Pull.
        let worker_delivery_mode = dispatcher
            .as_any()
            .downcast_ref::<MockDispatcher>()
            .map(|d| match d.mode() {
                MockDispatcherMode::Push => CommandDeliveryMode::Push,
                MockDispatcherMode::Pull => CommandDeliveryMode::Pull,
            })
            .unwrap_or(CommandDeliveryMode::Pull);

        // Auto-register a default command target so single-target shorthand
        // resolution works out of the box: a Push-mode server gets a Worker
        // (the only push-capable target type), a Pull-mode server a Daemon.
        let registry = Arc::new(InMemoryCommandRegistry::with_worker_delivery_mode(
            worker_delivery_mode,
        ));
        let default_target = match worker_delivery_mode {
            CommandDeliveryMode::Push => {
                CommandTarget::new("test-worker", CommandTargetType::Worker)
            }
            CommandDeliveryMode::Pull => {
                CommandTarget::new("test-daemon", CommandTargetType::Daemon)
            }
        };
        registry
            .register_target(
                default_target.resource_id.clone(),
                default_target.resource_type,
            )
            .await
            .expect("default target id is well-formed");

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

        // When fault injection is requested, the CommandServer talks to the KV
        // through the decorator while the harness keeps the inner LocalKv handle
        // (for inspection) and exposes the decorator (for arming faults).
        let (kv_for_server, fault_kv): (Arc<dyn Kv>, Option<Arc<FaultInjectingKv>>) =
            if self.fault_injection {
                let wrapper = Arc::new(FaultInjectingKv::new(kv.clone()));
                (wrapper.clone() as Arc<dyn Kv>, Some(wrapper))
            } else {
                (kv.clone() as Arc<dyn Kv>, None)
            };

        let mut command_server = CommandServer::new(
            kv_for_server,
            storage.clone() as Arc<dyn Storage>,
            dispatcher.clone(),
            registry.clone(),
            command_base_url,
            b"test-signing-key-for-commands".to_vec(),
        );
        if let Some(max) = self.max_open_lease_waits {
            command_server = command_server.with_max_open_lease_waits(max);
        }
        if let Some(max) = self.max_open_lease_waits_per_target {
            command_server = command_server.with_max_open_lease_waits_per_target(max);
        }
        if let Some(max) = self.max_open_lease_waits_per_deployment {
            command_server = command_server.with_max_open_lease_waits_per_deployment(max);
        }
        if let Some(interval) = self.lease_wait_probe_interval {
            command_server = command_server.with_lease_wait_probe_interval(interval);
        }
        let command_server = Arc::new(command_server);

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
            registry,
            default_target,
            fault_kv,
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
