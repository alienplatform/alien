//! Container App Transport
//!
//! Receives work via HTTP with Dapr integration:
//! - HTTP requests → forwarded to app's HTTP server
//! - Blob CloudEvent → StorageEvent via gRPC
//! - Service Bus (Dapr) → QueueMessage via gRPC (or Command if command envelope)
//! - Timer trigger → CronEvent via gRPC

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::providers::queue::storage_notifications::azure_queue_payload;
use alien_worker_protocol::ControlGrpcServer;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{Method, Request, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use chrono::Utc;
use cloudevents::AttributesReader;

use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use super::shared::{
    create_forward_client, dispatch_queue_messages, forward_http_request,
    parse_cloudevent_from_http_with_extensions, process_received_command, send_cron_event,
    send_queue_message, send_storage_events, serve_with_bounded_shutdown,
};
use crate::error::{ErrorData, Result};
use crate::events::azure::{
    azure_storage_cloudevent_to_storage_events, dapr_cloudevent_to_queue_messages,
};
use alien_error::{Context, IntoAlienError};

/// Container App transport
pub struct ContainerAppTransport {
    port: u16,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    command_timeout: Duration,
    http_shutdown_grace: Duration,
    shutdown_rx: broadcast::Receiver<()>,
}

const DEFAULT_HTTP_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

impl ContainerAppTransport {
    pub fn new(
        port: u16,
        control_server: Arc<ControlGrpcServer>,
        shutdown_rx: broadcast::Receiver<()>,
    ) -> Self {
        Self {
            port,
            control_server,
            app_http_port: None,
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

    /// Run the transport
    pub async fn run(self) -> Result<()> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));

        info!(port = self.port, "Starting Container App transport");

        let proxy_shutdown = CancellationToken::new();
        let collectors =
            super::queue_batch::QueueCollectors::from_env(self.control_server.clone())?;
        let state = TransportState {
            received_at: tokio::time::Instant::now(),
            collector: collectors.clients.clone(),
            control_server: self.control_server,
            app_http_port: self.app_http_port,
            command_timeout: self.command_timeout,
            http_client: create_forward_client(),
            proxy_shutdown: proxy_shutdown.clone(),
        };

        let app = Router::new()
            .route("/{*path}", any(handle_request))
            .route("/", any(handle_request))
            .with_state(state);

        let listener = TcpListener::bind(addr).await.into_alien_error().context(
            ErrorData::TransportStartupFailed {
                transport_name: "containerapp".to_string(),
                message: format!("Failed to bind to {}", addr),
                address: Some(addr.to_string()),
            },
        )?;

        info!(addr = %addr, "Container App transport listening");

        let server_result = serve_with_bounded_shutdown(
            listener,
            app,
            self.shutdown_rx,
            proxy_shutdown,
            self.http_shutdown_grace,
            "containerapp",
            || collectors.stop(),
        )
        .await;
        let collector_result = collectors.join().await;
        collector_result?;
        server_result
            .into_alien_error()
            .context(ErrorData::TransportStartupFailed {
                transport_name: "containerapp".to_string(),
                message: "Server error".to_string(),
                address: Some(addr.to_string()),
            })?;

        info!("Container App transport shutdown complete");
        Ok(())
    }
}

#[derive(Clone)]
struct TransportState {
    received_at: tokio::time::Instant,
    collector: std::collections::HashMap<String, super::queue_batch::QueueCollector>,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    command_timeout: Duration,
    http_client: reqwest::Client,
    proxy_shutdown: CancellationToken,
}

impl TransportState {
    fn remaining_timeout(&self) -> Duration {
        self.command_timeout
            .saturating_sub(self.received_at.elapsed())
    }
}

async fn handle_request(
    State(mut state): State<TransportState>,
    request: Request<Body>,
) -> impl IntoResponse {
    state.received_at = tokio::time::Instant::now();
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    // Dapr pauses input bindings while this probe fails, including before the
    // application subscribes and after cancellation. Do not proxy app health:
    // an HTTP server can be alive while its task stream is disconnected.
    if path == "/__alien/health" {
        return if state.control_server.task_application_ready()
            && !state.proxy_shutdown.is_cancelled()
        {
            StatusCode::NO_CONTENT
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        }
        .into_response();
    }

    debug!(path = %path, method = %method, "Received request");

    // Check for Azure Timer trigger (Container Apps Jobs)
    let is_timer_trigger =
        request.headers().get("X-Azure-Timer").is_some() || path.starts_with("/api/timer");
    let is_dapr_cron = path.starts_with("/cron-");
    let is_dapr_input_binding = is_dapr_cron
        || path.starts_with("/__alien/queue/")
        || path.starts_with("/servicebus-")
        || path.starts_with("/blobstorage-");

    // Check for CloudEvents (Azure Blob events)
    let ce_type = request
        .headers()
        .get("ce-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Check for Dapr messages: input bindings POST to /{component-name},
    // pubsub delivers with dapr-content-type header or /pubsub/ path. Dapr also
    // wraps Service Bus pub/sub deliveries as a bare CloudEvent whose `ce-type`
    // is the fixed value "com.dapr.event.sent" (see
    // `events::azure::dapr_cloudevent_to_queue_messages`), with no other
    // Dapr-specific header or path marker present — that must route here too,
    // not fall through to the blob-only CloudEvent handler.
    let is_dapr = path.starts_with("/__alien/queue/")
        || request.headers().get("dapr-content-type").is_some()
        || path.contains("/pubsub/")
        || path.starts_with("/servicebus-")
        || path.starts_with("/blobstorage-")
        || ce_type.as_deref() == Some("com.dapr.event.sent");

    if is_timer_trigger {
        return handle_timer_trigger(request, &state).await;
    }

    // Dapr probes each input binding's component-named endpoint with OPTIONS.
    // A successful response opts the application into receiving binding events.
    if is_dapr_input_binding && method == Method::OPTIONS {
        return StatusCode::NO_CONTENT.into_response();
    }
    if is_dapr_input_binding && method != Method::POST {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }

    if is_dapr_cron {
        return handle_dapr_cron_trigger(&path, &state).await;
    }

    if is_dapr {
        return handle_dapr_message(request, &state).await;
    }

    if let Some(event_type) = ce_type {
        return handle_cloudevent(request, &event_type, &state).await;
    }

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

/// Handle Dapr cron binding delivery at its component-named endpoint.
async fn handle_dapr_cron_trigger(path: &str, state: &TransportState) -> Response<Body> {
    let schedule_name = path.trim_start_matches('/').to_string();
    info!(%schedule_name, "Dapr cron trigger received");
    send_cron_event(
        schedule_name,
        Utc::now(),
        &state.control_server,
        state.remaining_timeout(),
    )
    .await
}

/// Handle Azure Timer trigger events
async fn handle_timer_trigger(request: Request<Body>, state: &TransportState) -> Response<Body> {
    let schedule_name = request
        .headers()
        .get("X-Azure-Timer-Name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("azure-timer")
        .to_string();

    let schedule_time = request
        .headers()
        .get("X-Azure-Timer-ScheduledTime")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    info!(schedule_name = %schedule_name, "Azure Timer trigger received");

    send_cron_event(
        schedule_name,
        schedule_time,
        &state.control_server,
        state.remaining_timeout(),
    )
    .await
}

/// Handle Dapr messages (input bindings and pub/sub from Service Bus)
async fn handle_dapr_message(request: Request<Body>, state: &TransportState) -> Response<Body> {
    debug!("Processing Dapr message");

    if request.uri().path().starts_with("/blobstorage-") {
        return handle_dapr_storage_message(request, state).await;
    }

    // Collect body for CloudEvent parsing
    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 4 * 1024 * 1024).await {
        Ok(collected) => collected,
        Err(e) => {
            error!(error = %e, "Failed to read request body");
            return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
        }
    };

    // Parse CloudEvent from Dapr (with extension headers)
    let cloud_event = match parse_cloudevent_from_http_with_extensions(&parts.headers, &body_bytes)
    {
        Ok(event) => event,
        Err(e) => {
            warn!(error = %e, "Failed to parse CloudEvent from Dapr, treating as raw message");
            // Try to handle as raw JSON message
            return handle_raw_dapr_message(
                &body_bytes,
                state,
                parts.uri.path().strip_prefix("/__alien/queue/"),
            )
            .await;
        }
    };

    // Event Grid sends a complete CloudEvent as the Service Bus message body.
    // Keep notifications delivered to a queue route in the queue dispatcher.
    if cloud_event.ty() == "Microsoft.Storage.BlobCreated" {
        if let Some(logical) = parts.uri.path().strip_prefix("/__alien/queue/") {
            return handle_raw_dapr_message(&body_bytes, state, Some(logical)).await;
        }
    }

    // Process as Dapr CloudEvent (Service Bus message)
    match dapr_cloudevent_to_queue_messages(cloud_event) {
        Ok(mut queue_messages) => {
            if let Some(logical) = parts.uri.path().strip_prefix("/__alien/queue/") {
                for message in &mut queue_messages {
                    message.source = if state.collector.contains_key(logical) {
                        logical.to_owned()
                    } else {
                        super::shared::single_queue_source(
                            &state.control_server,
                            &message.source,
                            logical,
                        )
                        .await
                    };
                }
            }
            dispatch_queue_messages(
                queue_messages,
                &state.control_server,
                state.remaining_timeout(),
                &state.collector,
            )
            .await
        }
        Err(e) => {
            error!(error = %e, "Failed to parse Dapr CloudEvent");
            (StatusCode::BAD_REQUEST, "Invalid Dapr event").into_response()
        }
    }
}

/// Handle an Event Grid CloudEvent delivered through a Service Bus Dapr input binding.
async fn handle_dapr_storage_message(
    request: Request<Body>,
    state: &TransportState,
) -> Response<Body> {
    let body_bytes = match axum::body::to_bytes(request.into_body(), 4 * 1024 * 1024).await {
        Ok(collected) => collected,
        Err(error) => {
            error!(%error, "Failed to read Dapr storage-trigger request body");
            return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
        }
    };

    let json: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(json) => json,
        Err(error) => {
            error!(%error, "Failed to parse Dapr storage-trigger body as JSON");
            return (StatusCode::BAD_REQUEST, "Invalid storage CloudEvent").into_response();
        }
    };

    let cloud_events = if json.is_array() {
        serde_json::from_value::<Vec<cloudevents::Event>>(json)
    } else {
        serde_json::from_value::<cloudevents::Event>(json).map(|event| vec![event])
    };
    let cloud_events = match cloud_events {
        Ok(events) if !events.is_empty() => events,
        Ok(_) => {
            return (StatusCode::BAD_REQUEST, "Empty storage CloudEvent batch").into_response();
        }
        Err(error) => {
            error!(%error, "Failed to decode Dapr storage-trigger CloudEvent");
            return (StatusCode::BAD_REQUEST, "Invalid storage CloudEvent").into_response();
        }
    };

    for cloud_event in cloud_events {
        let event_type = cloud_event.ty().to_string();
        let storage_events = match azure_storage_cloudevent_to_storage_events(cloud_event) {
            Ok(storage_events) => storage_events,
            Err(error) => {
                error!(%error, %event_type, "Failed to parse Dapr Azure storage CloudEvent");
                return (StatusCode::BAD_REQUEST, "Invalid storage event").into_response();
            }
        };
        let response = send_storage_events(
            storage_events,
            &state.control_server,
            state.remaining_timeout(),
        )
        .await;
        if !response.status().is_success() {
            return response;
        }
    }

    StatusCode::OK.into_response()
}

/// Handle raw Dapr message (input binding or non-CloudEvent format).
///
/// Dapr input bindings POST the raw message body (not wrapped in CloudEvent).
/// We first check if the body is a command envelope; if not, treat as queue message.
async fn handle_raw_dapr_message(
    body_bytes: &Bytes,
    state: &TransportState,
    logical_source: Option<&str>,
) -> Response<Body> {
    // Parse as JSON
    let json_value: serde_json::Value = match serde_json::from_slice(body_bytes) {
        Ok(v) => v,
        Err(e) => {
            error!(error = %e, "Failed to parse raw Dapr message as JSON");
            return (StatusCode::BAD_REQUEST, "Invalid JSON").into_response();
        }
    };

    // Check if this is a command envelope (has commandId + command fields)
    if let Ok(envelope) = serde_json::from_value::<alien_commands::Envelope>(json_value.clone()) {
        if !envelope.command_id.is_empty() {
            info!(command_id = %envelope.command_id, "Received command via Dapr input binding");
            if process_received_command(&envelope, &state.control_server, state.remaining_timeout())
                .await
            {
                return StatusCode::OK.into_response();
            }
            return (StatusCode::BAD_REQUEST, "Failed to decode command").into_response();
        }
    }

    // Not a command — treat as regular queue message
    let message_id = json_value
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let original_source = json_value
        .get("topic")
        .or_else(|| json_value.get("pubsubname"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let source = match logical_source {
        Some(logical) if state.collector.contains_key(logical) => logical.to_owned(),
        Some(logical) => {
            super::shared::single_queue_source(&state.control_server, original_source, logical)
                .await
        }
        None => original_source.to_owned(),
    };

    let payload = azure_queue_payload(alien_core::MessagePayload::Json(json_value));

    let qm = alien_core::QueueMessage {
        id: message_id,
        payload,
        receipt_handle: String::new(),
        timestamp: Utc::now(),
        source,
        attributes: std::collections::HashMap::new(),
        attempt_count: None,
    };

    if let Some(collector) = state.collector.get(&qm.source) {
        return if collector
            .deliver(
                super::shared::queue_message_to_proto(&qm),
                tokio::time::Instant::now() + state.remaining_timeout(),
            )
            .await
        {
            StatusCode::OK.into_response()
        } else {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Queue delivery not completed",
            )
                .into_response()
        };
    }
    if let Err(e) = send_queue_message(&qm, &state.control_server, state.remaining_timeout()).await
    {
        error!(error = %e, "Failed to send raw Dapr message to application");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to process message",
        )
            .into_response();
    }

    StatusCode::OK.into_response()
}

/// Handle CloudEvents (Azure Blob Storage events)
async fn handle_cloudevent(
    request: Request<Body>,
    event_type: &str,
    state: &TransportState,
) -> Response<Body> {
    debug!(event_type = %event_type, "Processing CloudEvent");

    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 4 * 1024 * 1024).await {
        Ok(collected) => collected,
        Err(e) => {
            error!(error = %e, "Failed to read request body");
            return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
        }
    };

    let cloud_event = match parse_cloudevent_from_http_with_extensions(&parts.headers, &body_bytes)
    {
        Ok(event) => event,
        Err(e) => {
            error!(error = %e, "Failed to parse CloudEvent");
            return (StatusCode::BAD_REQUEST, "Invalid CloudEvent").into_response();
        }
    };

    // Handle Azure Blob Storage events
    if event_type.starts_with("Microsoft.Storage.Blob") {
        return handle_azure_storage_cloudevent(cloud_event, state).await;
    }

    warn!(event_type = %event_type, "Unsupported CloudEvent type");
    StatusCode::OK.into_response()
}

/// Handle Azure Blob Storage CloudEvents
async fn handle_azure_storage_cloudevent(
    cloud_event: cloudevents::Event,
    state: &TransportState,
) -> Response<Body> {
    let event_type = cloud_event.ty().to_string();

    match azure_storage_cloudevent_to_storage_events(cloud_event) {
        Ok(storage_events) => {
            send_storage_events(
                storage_events,
                &state.control_server,
                state.remaining_timeout(),
            )
            .await
        }
        Err(e) => {
            error!(error = %e, event_type = %event_type, "Failed to parse Azure storage CloudEvent");
            (StatusCode::BAD_REQUEST, "Invalid storage event").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_worker_protocol::control::{
        control_service_server::ControlService, queue_delivery_outcome::Disposition,
        send_task_result_request, task::Payload, QueueBatchResult, QueueDeliveryOutcome,
        SendTaskResultRequest, WaitForTasksRequest,
    };
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn single_dapr_routes_preserve_existing_sources() {
        for registration in ["physical-jobs", "jobs", "*"] {
            let control = Arc::new(ControlGrpcServer::new());
            control
                .register_event_handler(tonic::Request::new(
                    alien_worker_protocol::control::RegisterEventHandlerRequest {
                        handler_type: "queue".into(),
                        resource_name: registration.into(),
                    },
                ))
                .await
                .expect("register");
            let mut tasks = control
                .wait_for_tasks(tonic::Request::new(WaitForTasksRequest {
                    application_id: "worker".into(),
                }))
                .await
                .expect("subscribe")
                .into_inner();
            let state = TransportState {
                received_at: tokio::time::Instant::now(),
                collector: Default::default(),
                control_server: control.clone(),
                app_http_port: None,
                command_timeout: Duration::from_secs(30),
                http_client: create_forward_client(),
                proxy_shutdown: CancellationToken::new(),
            };
            let request = Request::builder()
                .method("POST")
                .uri("/__alien/queue/jobs")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"id":"provider-id","topic":"physical-jobs"}"#,
                ))
                .expect("request");
            let response =
                tokio::spawn(
                    async move { handle_request(State(state), request).await.into_response() },
                );
            let task = tokio::time::timeout(
                Duration::from_secs(2),
                tokio_stream::StreamExt::next(&mut tasks),
            )
            .await
            .expect("dispatch")
            .expect("stream")
            .expect("task");
            let Some(alien_worker_protocol::control::task::Payload::QueueMessage(message)) =
                task.payload
            else {
                panic!("single expected")
            };
            assert_eq!(
                message.source,
                if registration == "jobs" {
                    "jobs"
                } else {
                    "physical-jobs"
                }
            );
            assert_eq!(message.id, "provider-id");
            assert!(!response.is_finished());
            control.send_task_result(tonic::Request::new(alien_worker_protocol::control::SendTaskResultRequest {
                task_id: task.task_id,
                result: Some(alien_worker_protocol::control::send_task_result_request::Result::Success(
                    alien_worker_protocol::control::TaskSuccess { response_data: Vec::new() },
                )),
            })).await.expect("result");
            assert!(response.await.expect("response").status().is_success());
        }
    }

    async fn health(state: &TransportState) -> StatusCode {
        handle_request(
            State(state.clone()),
            Request::builder()
                .uri("/__alien/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .into_response()
        .status()
    }

    #[tokio::test]
    async fn dapr_health_tracks_subscription_disconnect_cancellation_and_shutdown() {
        let control = Arc::new(ControlGrpcServer::new());
        let state = TransportState {
            received_at: tokio::time::Instant::now(),
            collector: Default::default(),
            control_server: control.clone(),
            app_http_port: None,
            command_timeout: Duration::from_secs(600),
            http_client: create_forward_client(),
            proxy_shutdown: CancellationToken::new(),
        };
        assert_eq!(health(&state).await, StatusCode::SERVICE_UNAVAILABLE);
        let stream = control
            .wait_for_tasks(tonic::Request::new(WaitForTasksRequest {
                application_id: "worker".into(),
            }))
            .await
            .unwrap();
        assert_eq!(health(&state).await, StatusCode::NO_CONTENT);
        drop(stream);
        assert_eq!(health(&state).await, StatusCode::SERVICE_UNAVAILABLE);
        let _stream = control
            .wait_for_tasks(tonic::Request::new(WaitForTasksRequest {
                application_id: "worker".into(),
            }))
            .await
            .unwrap();
        assert_eq!(health(&state).await, StatusCode::NO_CONTENT);
        state.proxy_shutdown.cancel();
        assert_eq!(health(&state).await, StatusCode::SERVICE_UNAVAILABLE);
        let state = TransportState {
            proxy_shutdown: CancellationToken::new(),
            ..state
        };
        assert_eq!(health(&state).await, StatusCode::NO_CONTENT);
        control.cancel_application().await;
        assert_eq!(health(&state).await, StatusCode::SERVICE_UNAVAILABLE);
        control.application_stopped();
        assert_eq!(health(&state).await, StatusCode::SERVICE_UNAVAILABLE);
    }
    #[tokio::test]
    async fn storage_notifications_remain_batched_queue_work() {
        for wrapped in [false, true] {
            let control = Arc::new(ControlGrpcServer::new());
            let mut tasks = control
                .wait_for_tasks(tonic::Request::new(WaitForTasksRequest {
                    application_id: "worker".into(),
                }))
                .await
                .unwrap()
                .into_inner();
            let environment = std::collections::HashMap::from([
            (
                "ALIEN_QUEUE_BATCH_CONFIG".into(),
                r#"{"jobs":{"maxMessages":2,"maxWindowSeconds":5}}"#.into(),
            ),
            (
                "ALIEN_JOBS_BINDING".into(),
                serde_json::json!({"service":"servicebus", "namespace":"test", "queueName":"jobs"})
                    .to_string(),
            ),
        ]);
            let collectors = super::super::queue_batch::QueueCollectors::from_environment(
                &environment,
                control.clone(),
            )
            .unwrap();
            let state = TransportState {
                received_at: tokio::time::Instant::now(),
                collector: collectors.clients.clone(),
                control_server: control.clone(),
                app_http_port: None,
                command_timeout: Duration::from_secs(30),
                http_client: create_forward_client(),
                proxy_shutdown: CancellationToken::new(),
            };
            let request = |key: &str| {
                let body = serde_json::json!({"id":key,"specversion":"1.0", "type":"Microsoft.Storage.BlobCreated", "source":"/test/storage", "time":"2026-10-30T08:00:00Z", "subject":format!("/blobServices/default/containers/source/blobs/{key}"), "data":{"contentLength":7,"eTag":"etag"}});
                let body = if wrapped {
                    serde_json::json!({
                        "specversion":"1.0", "id":key, "source":"test", "type":"com.dapr.event.sent",
                        "time":"2026-10-30T08:00:00Z", "datacontenttype":"application/json", "topic":"physical-jobs", "data":body,
                    })
                } else {
                    body
                };
                let builder = Request::builder()
                    .method("POST")
                    .uri("/__alien/queue/jobs")
                    .header(
                        "content-type",
                        if wrapped {
                            "application/cloudevents+json"
                        } else {
                            "application/json"
                        },
                    );

                builder.body(Body::from(body.to_string())).unwrap()
            };

            let first_state = state.clone();
            let first_request = request("in/first.json");
            let first = tokio::spawn(async move {
                handle_request(State(first_state), first_request)
                    .await
                    .into_response()
            });
            let second_request = request("in/second.json");
            let second = tokio::spawn(async move {
                handle_request(State(state), second_request)
                    .await
                    .into_response()
            });
            let task = tokio::time::timeout(Duration::from_secs(3), tasks.next())
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            let Some(Payload::QueueBatch(batch)) = task.payload else {
                panic!("storage notifications must dispatch a queue batch")
            };
            assert_eq!(batch.source, "jobs");
            assert_eq!(batch.deliveries.len(), 2);
            let mut keys = Vec::new();
            for delivery in &batch.deliveries {
                let message = delivery.message.as_ref().unwrap();
                assert_eq!(message.source, "jobs");
                let event: alien_core::StorageEvent =
                    serde_json::from_slice(&message.payload).unwrap();
                assert_eq!(event.event_type, alien_core::StorageEventType::Created);
                assert_eq!(event.bucket_name, "source");
                assert_eq!(event.size, Some(7));
                assert_eq!(event.etag.as_deref(), Some("etag"));
                keys.push(event.object_key);
            }
            keys.sort();
            assert_eq!(keys, ["in/first.json", "in/second.json"]);
            assert!(
                !first.is_finished() && !second.is_finished(),
                "enqueue must not acknowledge"
            );
            control
                .send_task_result(tonic::Request::new(SendTaskResultRequest {
                    task_id: task.task_id,
                    result: Some(send_task_result_request::Result::QueueBatch(
                        QueueBatchResult {
                            outcomes: batch
                                .deliveries
                                .iter()
                                .map(|delivery| QueueDeliveryOutcome {
                                    delivery_id: delivery.delivery_id.clone(),
                                    disposition: Disposition::Success as i32,
                                })
                                .collect(),
                        },
                    )),
                }))
                .await
                .unwrap();
            assert_eq!(first.await.unwrap().status(), StatusCode::OK);
            assert_eq!(second.await.unwrap().status(), StatusCode::OK);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), tasks.next())
                    .await
                    .is_err()
            );
            collectors.join().await.unwrap();
        }
    }
}
