//! Local Platform Transport
//!
//! A minimal HTTP proxy transport for the Local Platform. Forwards all HTTP
//! requests to the application's registered HTTP server without any
//! platform-specific middleware (no CloudEvents parsing, no scheduler handling).
//!
//! Used for:
//! - Local development (`acme run`)
//! - Production deployments on VMs, bare metal, edge devices
//! - Any environment where the runtime manages HTTP routing

use std::collections::HashSet;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_worker_protocol::ControlGrpcServer;
use axum::{
    body::Body,
    extract::{Json, State},
    http::{header::AUTHORIZATION, HeaderMap, Request, Response, StatusCode},
    response::IntoResponse,
    routing::{any, post},
    Router,
};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Semaphore};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;
use tracing::{debug, error, info};

use super::shared::{create_forward_client, forward_http_request, serve_with_bounded_shutdown};
use crate::error::{ErrorData, Result};
use alien_error::AlienError;

/// Local platform transport.
///
/// Simple HTTP proxy that forwards all requests to the application.
/// No CloudEvents parsing, no platform-specific middleware.
pub struct LocalTransport {
    bind_addr: [u8; 4],
    port: u16,
    transport_name: &'static str,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    command_push: Option<CommandPushConfig>,
    command_timeout: Duration,
    http_shutdown_grace: Duration,
    shutdown_rx: broadcast::Receiver<()>,
}

const DEFAULT_HTTP_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone)]
struct CommandPushConfig {
    token: String,
    deployment_id: String,
    worker_resource_id: String,
}

impl LocalTransport {
    pub fn new(
        port: u16,
        control_server: Arc<ControlGrpcServer>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            bind_addr: [127, 0, 0, 1],
            port,
            transport_name: "local",
            control_server,
            app_http_port: None,
            command_push: None,
            command_timeout: Duration::from_secs(300),
            http_shutdown_grace: DEFAULT_HTTP_SHUTDOWN_GRACE,
            shutdown_rx,
        }
    }

    pub fn exposed(
        port: u16,
        control_server: Arc<ControlGrpcServer>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            bind_addr: [0, 0, 0, 0],
            port,
            transport_name: "http",
            control_server,
            app_http_port: None,
            command_push: None,
            command_timeout: Duration::from_secs(300),
            http_shutdown_grace: DEFAULT_HTTP_SHUTDOWN_GRACE,
            shutdown_rx,
        }
    }

    pub fn with_app_port(mut self, port: u16) -> Self {
        self.app_http_port = Some(port);
        self
    }

    pub fn with_command_timeout(mut self, timeout: Duration) -> Self {
        self.command_timeout = timeout;
        self
    }

    #[cfg(test)]
    fn with_http_shutdown_grace(mut self, grace: Duration) -> Self {
        self.http_shutdown_grace = grace;
        self
    }

    /// Enable authenticated, resource-scoped command pushes on the
    /// runtime-owned endpoint.
    pub fn with_command_push(
        mut self,
        token: String,
        deployment_id: String,
        worker_resource_id: String,
    ) -> Self {
        self.command_push = Some(CommandPushConfig {
            token,
            deployment_id,
            worker_resource_id,
        });
        self
    }

    /// Run the transport.
    pub async fn run(self) -> Result<()> {
        let addr = SocketAddr::from((self.bind_addr, self.port));
        let transport_name = self.transport_name;

        info!(
            port = self.port,
            transport = transport_name,
            "Starting HTTP proxy transport"
        );

        let command_tasks = CommandTasks::new();
        let proxy_shutdown = CancellationToken::new();
        let state = TransportState {
            control_server: self.control_server,
            app_http_port: self.app_http_port,
            command_push: self.command_push,
            command_timeout: self.command_timeout,
            command_concurrency: Arc::new(Semaphore::new(1)),
            in_flight_commands: InFlightCommands::default(),
            command_tasks: command_tasks.clone(),
            http_client: create_forward_client(),
            proxy_shutdown: proxy_shutdown.clone(),
        };

        let app = Router::new()
            .route(
                alien_commands::WORKER_COMMAND_PUSH_PATH,
                post(handle_command_push),
            )
            .route("/{*path}", any(handle_request))
            .route("/", any(handle_request))
            .with_state(state);

        let listener = TcpListener::bind(addr).await.map_err(|e| {
            AlienError::new(ErrorData::TransportStartupFailed {
                transport_name: transport_name.to_string(),
                message: format!("Failed to bind to {}: {}", addr, e),
                address: Some(addr.to_string()),
            })
        })?;

        info!(
            addr = %addr,
            transport = transport_name,
            "HTTP proxy transport listening"
        );

        let command_tasks_on_shutdown = command_tasks.clone();
        let server_result = serve_with_bounded_shutdown(
            listener,
            app,
            self.shutdown_rx,
            proxy_shutdown,
            self.http_shutdown_grace,
            transport_name,
            move || {
                // Stop admitting command pushes, but do not cancel work that
                // has already received 202. The manager has durably marked
                // those commands Dispatched and will not redeliver them.
                command_tasks_on_shutdown.stop_accepting();
            },
        )
        .await;

        // Axum has finished all request handlers, so none can race a new
        // TaskTracker spawn with close/wait. Every command that received 202
        // retains its own execution and response-submission bounds and is
        // allowed to terminalize before the listener task returns.
        command_tasks.drain().await;

        server_result.map_err(|e| {
            AlienError::new(ErrorData::TransportStartupFailed {
                transport_name: transport_name.to_string(),
                message: format!("Server error: {}", e),
                address: Some(addr.to_string()),
            })
        })?;

        info!(
            transport = transport_name,
            "HTTP proxy transport shutdown complete"
        );
        Ok(())
    }
}

#[derive(Clone)]
struct TransportState {
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    command_push: Option<CommandPushConfig>,
    command_timeout: Duration,
    command_concurrency: Arc<Semaphore>,
    in_flight_commands: InFlightCommands,
    command_tasks: CommandTasks,
    http_client: reqwest::Client,
    proxy_shutdown: CancellationToken,
}

#[derive(Clone)]
struct CommandTasks {
    tracker: TaskTracker,
    accepting: Arc<AtomicBool>,
}

impl CommandTasks {
    fn new() -> Self {
        Self {
            tracker: TaskTracker::new(),
            accepting: Arc::new(AtomicBool::new(true)),
        }
    }

    fn is_accepting(&self) -> bool {
        self.accepting.load(Ordering::Acquire)
    }

    fn spawn<F>(&self, future: F) -> bool
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if !self.is_accepting() {
            return false;
        }

        self.tracker.spawn(future);
        true
    }

    fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::Release);
    }

    async fn drain(&self) {
        self.stop_accepting();
        self.tracker.close();
        self.tracker.wait().await;
    }
}

#[derive(Clone, Default)]
struct InFlightCommands {
    ids: Arc<Mutex<HashSet<String>>>,
}

impl InFlightCommands {
    fn try_start(&self, command_id: String) -> Option<InFlightCommandGuard> {
        let mut ids = self
            .ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !ids.insert(command_id.clone()) {
            return None;
        }
        Some(InFlightCommandGuard {
            command_id,
            ids: self.ids.clone(),
        })
    }
}

struct InFlightCommandGuard {
    command_id: String,
    ids: Arc<Mutex<HashSet<String>>>,
}

impl Drop for InFlightCommandGuard {
    fn drop(&mut self) {
        self.ids
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.command_id);
    }
}

async fn handle_command_push(
    State(state): State<TransportState>,
    headers: HeaderMap,
    Json(envelope): Json<alien_commands::Envelope>,
) -> Response<Body> {
    let Some(command_push) = state.command_push.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !command_push_authorized(&headers, &command_push.token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if envelope.validate().is_err() {
        return (StatusCode::BAD_REQUEST, "Invalid command envelope").into_response();
    }
    if !command_push_targets_runtime(
        &envelope,
        &command_push.deployment_id,
        &command_push.worker_resource_id,
    ) {
        return StatusCode::FORBIDDEN.into_response();
    }

    if !state.command_tasks.is_accepting() {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    let Some(in_flight) = state
        .in_flight_commands
        .try_start(envelope.command_id.clone())
    else {
        // The first accepted delivery owns execution and response submission.
        // A duplicate transport delivery is acknowledged but never re-run.
        return StatusCode::ACCEPTED.into_response();
    };

    let control_server = state.control_server.clone();
    let concurrency = state.command_concurrency.clone();
    let budget =
        super::shared::CommandBudget::from_envelope(state.command_timeout, envelope.deadline);
    if !state.command_tasks.spawn(async move {
        let _in_flight = in_flight;
        super::shared::process_pushed_command(envelope, control_server, concurrency, budget).await;
    }) {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    StatusCode::ACCEPTED.into_response()
}

fn command_push_authorized(headers: &HeaderMap, expected_token: &str) -> bool {
    let expected_authorization = format!("Bearer {expected_token}");
    headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected_authorization)
}

fn command_push_targets_runtime(
    envelope: &alien_commands::Envelope,
    expected_deployment_id: &str,
    expected_worker_resource_id: &str,
) -> bool {
    envelope.deployment_id == expected_deployment_id
        && envelope.target.resource_type == alien_core::CommandTargetType::Worker
        && envelope.target.resource_id == expected_worker_resource_id
}

async fn handle_request(
    State(state): State<TransportState>,
    request: Request<Body>,
) -> Response<Body> {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    debug!(path = %path, method = %method, "Received request");

    // Forward HTTP request to app
    if let Some(app_port) = state.app_http_port {
        return forward_http_request(
            &state.http_client,
            request,
            app_port,
            state.proxy_shutdown.clone(),
        )
        .await;
    }

    error!("No app HTTP port registered");
    (StatusCode::SERVICE_UNAVAILABLE, "No application registered").into_response()
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use alien_commands::{BodySpec, CommandTarget, CommandTargetType, Envelope};
    use alien_worker_protocol::control::{
        control_service_server::ControlService, send_task_result_request::Result as TaskResult,
        SendTaskResultRequest, TaskSuccess, WaitForTasksRequest,
    };
    use axum::{
        body::Bytes,
        extract::State as AxumState,
        routing::{get, put},
        Router as AxumRouter,
    };
    use futures_util::StreamExt;
    use tonic::Request as TonicRequest;

    use super::*;

    fn envelope(target: CommandTarget) -> Envelope {
        Envelope::new(
            "deployment",
            target,
            "command-id",
            1,
            None,
            "run",
            BodySpec::inline(b"{}"),
            alien_commands::test_utils::test_response_handling("command-id"),
        )
    }

    async fn record_terminal_response(AxumState(count): AxumState<Arc<AtomicUsize>>) -> StatusCode {
        count.fetch_add(1, Ordering::SeqCst);
        StatusCode::OK
    }

    async fn endless_stream() -> Response<Body> {
        let first = futures_util::stream::once(async {
            Ok::<_, Infallible>(Bytes::from_static(b"data: started\n\n"))
        });
        let pending = futures_util::stream::pending::<std::result::Result<Bytes, Infallible>>();
        Response::new(Body::from_stream(first.chain(pending)))
    }

    async fn unused_port() -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    async fn wait_for_port(port: u16) {
        tokio::time::timeout(Duration::from_secs(2), async move {
            loop {
                if tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .is_ok()
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("listener did not start");
    }

    #[test]
    fn command_push_requires_exact_bearer_token() {
        let mut headers = HeaderMap::new();
        assert!(!command_push_authorized(&headers, "secret"));

        headers.insert(AUTHORIZATION, "Bearer wrong".parse().unwrap());
        assert!(!command_push_authorized(&headers, "secret"));

        headers.insert(AUTHORIZATION, "Bearer secret".parse().unwrap());
        assert!(command_push_authorized(&headers, "secret"));
    }

    #[test]
    fn command_push_requires_exact_worker_target() {
        let expected = envelope(CommandTarget::new("reports", CommandTargetType::Worker));
        assert!(command_push_targets_runtime(
            &expected,
            "deployment",
            "reports"
        ));

        let another_worker = envelope(CommandTarget::new("billing", CommandTargetType::Worker));
        assert!(!command_push_targets_runtime(
            &another_worker,
            "deployment",
            "reports"
        ));

        for resource_type in [CommandTargetType::Container, CommandTargetType::Daemon] {
            let non_worker = envelope(CommandTarget::new("reports", resource_type));
            assert!(
                !command_push_targets_runtime(&non_worker, "deployment", "reports"),
                "a deployment token shared with {resource_type:?} must not authorize that target at a Worker endpoint"
            );
        }
    }

    #[test]
    fn command_push_requires_exact_deployment() {
        let wrong_deployment = envelope(CommandTarget::new("reports", CommandTargetType::Worker));

        assert!(!command_push_targets_runtime(
            &wrong_deployment,
            "another-deployment",
            "reports"
        ));
    }

    #[test]
    fn command_push_validation_preserves_protocol_check() {
        let mut invalid = envelope(CommandTarget::new("reports", CommandTargetType::Worker));
        invalid.protocol = "arc.v0".to_string();

        assert!(invalid.validate().is_err());
        assert!(command_push_targets_runtime(
            &invalid,
            "deployment",
            "reports"
        ));
    }

    #[test]
    fn duplicate_command_id_is_suppressed_until_the_owner_finishes() {
        let in_flight = InFlightCommands::default();
        let owner = in_flight
            .try_start("command-id".to_string())
            .expect("first delivery owns command");
        assert!(in_flight.try_start("command-id".to_string()).is_none());

        drop(owner);
        assert!(in_flight.try_start("command-id".to_string()).is_some());
    }

    #[tokio::test]
    async fn distinct_pushes_execute_under_one_runtime_permit() {
        let tasks = CommandTasks::new();
        let in_flight = InFlightCommands::default();
        let concurrency = Arc::new(Semaphore::new(1));
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));

        for command_id in ["first", "second"] {
            let guard = in_flight
                .try_start(command_id.to_string())
                .expect("distinct command accepted");
            let concurrency = concurrency.clone();
            let active = active.clone();
            let maximum = maximum.clone();
            let completed = completed.clone();
            assert!(tasks.spawn(async move {
                let _guard = guard;
                let _permit = concurrency.acquire().await.expect("semaphore open");
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(20)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                completed.fetch_add(1, Ordering::SeqCst);
            }));
        }

        tasks.tracker.close();
        tasks.tracker.wait().await;

        assert_eq!(completed.load(Ordering::SeqCst), 2);
        assert_eq!(maximum.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shutdown_drains_already_accepted_pushes() {
        let tasks = CommandTasks::new();
        let started = Arc::new(tokio::sync::Notify::new());
        let finish = Arc::new(tokio::sync::Notify::new());
        let completed = Arc::new(AtomicBool::new(false));
        let started_in_task = started.clone();
        let finish_in_task = finish.clone();
        let completed_in_task = completed.clone();
        assert!(tasks.spawn(async move {
            started_in_task.notify_one();
            finish_in_task.notified().await;
            completed_in_task.store(true, Ordering::SeqCst);
        }));

        started.notified().await;
        tasks.stop_accepting();
        assert!(!tasks.spawn(async {}));
        let drain = tasks.drain();
        tokio::pin!(drain);
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut drain)
                .await
                .is_err(),
            "drain must wait for accepted work"
        );
        finish.notify_one();
        drain.await;
        assert!(completed.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn accepted_push_terminalizes_during_transport_shutdown() {
        let response_count = Arc::new(AtomicUsize::new(0));
        let response_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let response_address = response_listener.local_addr().unwrap();
        let response_count_for_server = response_count.clone();
        let response_server = tokio::spawn(async move {
            axum::serve(
                response_listener,
                AxumRouter::new()
                    .route("/response", put(record_terminal_response))
                    .with_state(response_count_for_server),
            )
            .await
            .unwrap();
        });

        let control_server = Arc::new(ControlGrpcServer::new());
        let task_stream = control_server
            .wait_for_tasks(TonicRequest::new(WaitForTasksRequest {
                application_id: "app".to_string(),
            }))
            .await
            .unwrap()
            .into_inner();
        let control_for_app = control_server.clone();
        let app = tokio::spawn(async move {
            tokio::pin!(task_stream);
            let task = task_stream
                .next()
                .await
                .expect("command task")
                .expect("valid command task");
            // Make shutdown win while the already-accepted task is executing.
            tokio::time::sleep(Duration::from_millis(50)).await;
            control_for_app
                .send_task_result(TonicRequest::new(SendTaskResultRequest {
                    task_id: task.task_id,
                    result: Some(TaskResult::Success(TaskSuccess {
                        response_data: b"{\"ok\":true}".to_vec(),
                    })),
                }))
                .await
                .expect("submit app result");
        });

        let port = unused_port().await;
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let transport = LocalTransport::new(port, control_server, shutdown_rx)
            .with_command_timeout(Duration::from_secs(2))
            .with_command_push(
                "secret".to_string(),
                "deployment".to_string(),
                "reports".to_string(),
            );
        let transport_task = tokio::spawn(async move { transport.run().await });
        wait_for_port(port).await;

        let mut pushed = envelope(CommandTarget::new("reports", CommandTargetType::Worker));
        pushed.response_handling.submit_response_url =
            format!("http://{response_address}/response");
        let accepted = reqwest::Client::new()
            .post(format!(
                "http://127.0.0.1:{port}{}",
                alien_commands::WORKER_COMMAND_PUSH_PATH
            ))
            .bearer_auth("secret")
            .json(&pushed)
            .send()
            .await
            .expect("push command");
        assert_eq!(accepted.status(), StatusCode::ACCEPTED);

        shutdown_tx.send(()).expect("transport shutdown receiver");
        transport_task
            .await
            .expect("transport task")
            .expect("transport shutdown");
        app.await.expect("application task");
        assert_eq!(
            response_count.load(Ordering::SeqCst),
            1,
            "the accepted command must terminalize before transport exit"
        );
        response_server.abort();
    }

    #[tokio::test]
    async fn active_proxy_stream_cannot_block_transport_shutdown() {
        let app_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let app_port = app_listener.local_addr().unwrap().port();
        let app_server = tokio::spawn(async move {
            axum::serve(
                app_listener,
                AxumRouter::new().route("/stream", get(endless_stream)),
            )
            .await
            .unwrap();
        });

        let transport_port = unused_port().await;
        let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
        let transport = LocalTransport::new(
            transport_port,
            Arc::new(ControlGrpcServer::new()),
            shutdown_rx,
        )
        .with_app_port(app_port)
        .with_http_shutdown_grace(Duration::from_millis(30));
        let transport_task = tokio::spawn(async move { transport.run().await });
        wait_for_port(transport_port).await;

        let mut response = reqwest::Client::new()
            .get(format!("http://127.0.0.1:{transport_port}/stream"))
            .send()
            .await
            .expect("open proxied stream");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.chunk().await.expect("read first stream chunk"),
            Some(Bytes::from_static(b"data: started\n\n"))
        );

        shutdown_tx.send(()).expect("transport shutdown receiver");
        tokio::time::timeout(Duration::from_secs(1), transport_task)
            .await
            .expect("active stream must not block transport shutdown")
            .expect("transport task")
            .expect("transport shutdown");

        tokio::time::timeout(Duration::from_secs(1), response.chunk())
            .await
            .expect("proxied stream must close after shutdown")
            .ok();
        app_server.abort();
    }

    #[tokio::test]
    async fn shutdown_spawn_race_drops_future_and_releases_in_flight_id() {
        let tasks = CommandTasks::new();
        let in_flight = InFlightCommands::default();
        let guard = in_flight
            .try_start("command-id".to_string())
            .expect("command accepted before shutdown");
        let ran = Arc::new(AtomicBool::new(false));
        let ran_in_task = ran.clone();

        tasks.stop_accepting();
        assert!(!tasks.spawn(async move {
            let _guard = guard;
            ran_in_task.store(true, Ordering::SeqCst);
        }));
        tokio::task::yield_now().await;

        assert!(!ran.load(Ordering::SeqCst));
        assert!(in_flight.try_start("command-id".to_string()).is_some());
    }
}
