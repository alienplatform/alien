//! Cloud Run Transport
//!
//! Receives work via HTTP with CloudEvents:
//! - HTTP requests → forwarded to app's HTTP server
//! - GCS CloudEvent → StorageEvent via gRPC
//! - Pub/Sub CloudEvent → QueueMessage via gRPC (or Command if command envelope)
//! - Cloud Scheduler → CronEvent via gRPC

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use alien_bindings::providers::queue::storage_notifications::{
    gcp_queue_payload, GcpQueueDelivery,
};
use alien_worker_protocol::ControlGrpcServer;
use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use cloudevents::AttributesReader;

use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use super::shared::{
    create_forward_client, dispatch_queue_messages, forward_http_request,
    parse_cloudevent_from_http, send_cron_event, send_storage_events, serve_with_bounded_shutdown,
};
use crate::error::{ErrorData, Result};
use crate::events::gcp::{
    gcs_pubsub_notification_to_storage_events, pubsub_cloudevent_to_queue_messages_with_source,
    storage_cloudevent_to_storage_events,
};
use alien_error::{Context, IntoAlienError};

/// Cloud Run transport
pub struct CloudRunTransport {
    port: u16,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    command_timeout: Duration,
    http_shutdown_grace: Duration,
    shutdown_rx: broadcast::Receiver<()>,
}

const DEFAULT_HTTP_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

impl CloudRunTransport {
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

        info!(port = self.port, "Starting Cloud Run transport");

        let proxy_shutdown = CancellationToken::new();
        let collectors =
            super::queue_batch::QueueCollectors::from_env(self.control_server.clone())?;
        let state = TransportState {
            received_at: tokio::time::Instant::now(),
            collector: collectors.clients.clone(),
            control_server: self.control_server,
            app_http_port: self.app_http_port,
            // Push acknowledgments cannot be extended past Pub/Sub's 600s
            // deadline. Leave time for response submission and HTTP completion.
            command_timeout: self.command_timeout.min(Duration::from_secs(565)),
            http_client: create_forward_client(),
            proxy_shutdown: proxy_shutdown.clone(),
        };

        let app = Router::new()
            .route("/{*path}", any(handle_request))
            .route("/", any(handle_request))
            .with_state(state);

        let listener = TcpListener::bind(addr).await.into_alien_error().context(
            ErrorData::TransportStartupFailed {
                transport_name: "cloudrun".to_string(),
                message: format!("Failed to bind to {}", addr),
                address: Some(addr.to_string()),
            },
        )?;

        info!(addr = %addr, "Cloud Run transport listening");

        let server_result = serve_with_bounded_shutdown(
            listener,
            app,
            self.shutdown_rx,
            proxy_shutdown,
            self.http_shutdown_grace,
            "cloudrun",
            || collectors.stop(),
        )
        .await;
        let collector_result = collectors.join().await;
        collector_result?;
        server_result
            .into_alien_error()
            .context(ErrorData::TransportStartupFailed {
                transport_name: "cloudrun".to_string(),
                message: "Server error".to_string(),
                address: Some(addr.to_string()),
            })?;

        info!("Cloud Run transport shutdown complete");
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

    debug!(path = %path, method = %method, "Received request");

    // Check for Cloud Scheduler header
    let is_scheduler = request.headers().get("X-CloudScheduler").is_some()
        || request.headers().get("X-CloudScheduler-JobName").is_some();

    // Check for CloudEvents
    let ce_type = request
        .headers()
        .get("ce-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    if is_scheduler {
        return handle_scheduler_event(request, &state).await;
    }

    if let Some(event_type) = ce_type {
        return handle_cloudevent(request, &event_type, &state).await;
    }

    // Check for Pub/Sub push message (non-CloudEvent format).
    // Pub/Sub push subscriptions send POST requests with a JSON body containing
    // "message" and "subscription" fields, without CloudEvent headers.
    if method == axum::http::Method::POST && (path == "/" || path.starts_with("/__alien/queue/")) {
        // Peek at the body to check for PubSub push format
        let (parts, body) = request.into_parts();
        // The legacy root also carries arbitrary application HTTP uploads. Preserve
        // its existing size policy; the explicit queue route has a bounded envelope.
        let limit = if path == "/" {
            usize::MAX
        } else {
            4 * 1024 * 1024
        };
        let body_bytes = match axum::body::to_bytes(body, limit).await {
            Ok(collected) => collected,
            Err(e) => {
                error!(error = %e, "Failed to read request body");
                return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
            }
        };

        if let Ok(push_msg) = serde_json::from_slice::<PubSubPushMessage>(&body_bytes) {
            if push_msg.subscription.is_some() {
                debug!(
                    subscription = ?push_msg.subscription,
                    "Detected Pub/Sub push message"
                );
                return handle_pubsub_push_message(
                    push_msg,
                    &state,
                    path.strip_prefix("/__alien/queue/"),
                )
                .await;
            }
        }

        // Not a PubSub push message — reconstruct request and forward to app
        let request = Request::from_parts(parts, Body::from(body_bytes));
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
        return (StatusCode::SERVICE_UNAVAILABLE, "No application registered").into_response();
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

/// Handle Cloud Scheduler events
async fn handle_scheduler_event(request: Request<Body>, state: &TransportState) -> Response<Body> {
    let job_name = request
        .headers()
        .get("X-CloudScheduler-JobName")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    let schedule_time = request
        .headers()
        .get("X-CloudScheduler-ScheduleTime")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    info!(job_name = %job_name, "Cloud Scheduler event received");

    send_cron_event(
        job_name,
        schedule_time,
        &state.control_server,
        state.remaining_timeout(),
    )
    .await
}

/// Handle CloudEvents (GCS storage events, Pub/Sub messages)
async fn handle_cloudevent(
    request: Request<Body>,
    event_type: &str,
    state: &TransportState,
) -> Response<Body> {
    debug!(event_type = %event_type, "Processing CloudEvent");

    // Collect body for CloudEvent parsing
    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, 4 * 1024 * 1024).await {
        Ok(collected) => collected,
        Err(e) => {
            error!(error = %e, "Failed to read request body");
            return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
        }
    };

    // Parse CloudEvent from HTTP request
    let cloud_event = match parse_cloudevent_from_http(&parts.headers, &body_bytes) {
        Ok(event) => event,
        Err(e) => {
            error!(error = %e, "Failed to parse CloudEvent");
            return (StatusCode::BAD_REQUEST, "Invalid CloudEvent").into_response();
        }
    };

    // Handle based on event type
    if event_type.starts_with("google.cloud.storage.object.v1.") {
        return handle_storage_cloudevent(cloud_event, state).await;
    }

    if event_type == "google.cloud.pubsub.topic.v1.messagePublished" {
        return handle_pubsub_cloudevent(
            cloud_event,
            state,
            parts.uri.path().strip_prefix("/__alien/queue/"),
        )
        .await;
    }

    warn!(event_type = %event_type, "Unsupported CloudEvent type");
    StatusCode::OK.into_response()
}

/// Handle GCS storage CloudEvents
async fn handle_storage_cloudevent(
    cloud_event: cloudevents::Event,
    state: &TransportState,
) -> Response<Body> {
    let event_type = cloud_event.ty().to_string();

    match storage_cloudevent_to_storage_events(cloud_event) {
        Ok(storage_events) => {
            send_storage_events(
                storage_events,
                &state.control_server,
                state.remaining_timeout(),
            )
            .await
        }
        Err(e) => {
            error!(error = %e, event_type = %event_type, "Failed to parse storage CloudEvent");
            (StatusCode::BAD_REQUEST, "Invalid storage event").into_response()
        }
    }
}

/// Handle Pub/Sub CloudEvents - may contain command envelope or regular queue message
async fn handle_pubsub_cloudevent(
    cloud_event: cloudevents::Event,
    state: &TransportState,
    logical_source: Option<&str>,
) -> Response<Body> {
    match pubsub_cloudevent_to_queue_messages_with_source(cloud_event, |subscription| {
        logical_source
            .filter(|source| state.collector.contains_key(*source))
            .map(str::to_owned)
            .or_else(|| {
                state
                    .collector
                    .contains_key(subscription)
                    .then(|| subscription.to_owned())
            })
    }) {
        Ok(mut queue_messages) => {
            if let Some(logical) = logical_source {
                if !state.collector.contains_key(logical) {
                    for message in &mut queue_messages {
                        message.source = super::shared::single_queue_source(
                            &state.control_server,
                            &message.source,
                            logical,
                        )
                        .await;
                    }
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
            error!(error = %e, "Failed to parse Pub/Sub CloudEvent");
            (StatusCode::BAD_REQUEST, "Invalid Pub/Sub event").into_response()
        }
    }
}

/// Pub/Sub push message format (non-CloudEvent)
#[derive(serde::Deserialize)]
struct PubSubPushMessage {
    message: PubSubPushMessageData,
    subscription: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PubSubPushMessageData {
    /// Base64-encoded message data
    data: Option<String>,
    /// Message attributes
    attributes: Option<std::collections::HashMap<String, String>>,
    /// Message ID
    message_id: Option<String>,
    /// Message publish time
    publish_time: Option<chrono::DateTime<Utc>>,
}

/// Handle a Pub/Sub push message (non-CloudEvent format)
async fn handle_pubsub_push_message(
    push_msg: PubSubPushMessage,
    state: &TransportState,
    logical_source: Option<&str>,
) -> Response<Body> {
    let data = match &push_msg.message.data {
        Some(d) => match general_purpose::STANDARD.decode(d) {
            Ok(decoded) => decoded,
            Err(e) => {
                error!(error = %e, "Failed to decode Pub/Sub message data");
                return (StatusCode::BAD_REQUEST, "Invalid message data").into_response();
            }
        },
        None => Vec::new(),
    };

    let attributes = push_msg.message.attributes.unwrap_or_default();
    // A GCS envelope on a queue route is still queue work. Only the legacy
    // dedicated storage subscription dispatches StorageEvent tasks directly.
    let queue_route = logical_source.is_some()
        || push_msg
            .subscription
            .as_ref()
            .is_some_and(|source| state.collector.contains_key(source));
    if !queue_route {
        match gcs_pubsub_notification_to_storage_events(
            &data,
            &attributes,
            push_msg.message.publish_time,
        ) {
            Ok(Some(storage_events)) => {
                return send_storage_events(
                    storage_events,
                    &state.control_server,
                    state.remaining_timeout(),
                )
                .await;
            }
            Ok(None) => {}
            Err(error) => {
                error!(error = %error, "Failed to parse GCS Pub/Sub notification");
                return (StatusCode::BAD_REQUEST, "Invalid storage event").into_response();
            }
        }
    }

    let message_id = push_msg
        .message
        .message_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let payload = match gcp_queue_payload(&data, &attributes) {
        GcpQueueDelivery::Deliver(payload) => payload,
        GcpQueueDelivery::Filtered => return StatusCode::OK.into_response(),
    };

    let original_source = push_msg.subscription.unwrap_or_default();
    let source = match logical_source {
        Some(logical) if state.collector.contains_key(logical) => logical.to_owned(),
        Some(logical) => {
            super::shared::single_queue_source(&state.control_server, &original_source, logical)
                .await
        }
        None => original_source,
    };

    let qm = alien_core::QueueMessage {
        id: message_id,
        payload,
        receipt_handle: String::new(),
        timestamp: Utc::now(),
        source,
        attributes,
        attempt_count: None,
    };

    dispatch_queue_messages(
        vec![qm],
        &state.control_server,
        state.remaining_timeout(),
        &state.collector,
    )
    .await
}

#[cfg(test)]
mod queue_routing_tests {
    use super::*;
    use alien_worker_protocol::control::{
        control_service_server::ControlService, queue_delivery_outcome::Disposition,
        send_task_result_request, task::Payload, QueueBatchResult, QueueDeliveryOutcome,
        SendTaskResultRequest, WaitForTasksRequest,
    };
    use std::collections::HashMap;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn large_root_upload_reaches_application_but_queue_route_stays_bounded() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen, mut received) = tokio::sync::mpsc::unbounded_channel();
        let app = Router::new().fallback(any(move |request: Request<Body>| {
            let seen = seen.clone();
            async move {
                let bytes = axum::body::to_bytes(request.into_body(), usize::MAX)
                    .await
                    .unwrap();
                seen.send(bytes.clone()).unwrap();
                (StatusCode::CREATED, bytes)
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let state = TransportState {
            received_at: tokio::time::Instant::now(),
            collector: HashMap::new(),
            control_server: Arc::new(ControlGrpcServer::new()),
            app_http_port: Some(port),
            command_timeout: Duration::from_secs(30),
            http_client: create_forward_client(),
            proxy_shutdown: CancellationToken::new(),
        };
        let upload =
            serde_json::to_vec(&serde_json::json!({"upload": "x".repeat(5 * 1024 * 1024)}))
                .unwrap();
        for (path, expected_status) in [
            ("/", StatusCode::CREATED),
            ("/__alien/queue/jobs", StatusCode::BAD_REQUEST),
        ] {
            let request = Request::builder()
                .method("POST")
                .uri(path)
                .header("content-type", "application/json")
                .body(Body::from(upload.clone()))
                .unwrap();
            let response = handle_request(State(state.clone()), request)
                .await
                .into_response();
            assert_eq!(response.status(), expected_status);
            if path == "/" {
                assert_eq!(
                    axum::body::to_bytes(response.into_body(), usize::MAX)
                        .await
                        .unwrap()
                        .as_ref(),
                    upload
                );
                assert_eq!(received.try_recv().unwrap().as_ref(), upload);
            } else {
                assert!(
                    received.try_recv().is_err(),
                    "oversized queue envelope must not reach the app"
                );
            }
        }
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }

    #[tokio::test]
    async fn single_pubsub_routes_preserve_existing_sources() {
        for cloud_event in [false, true] {
            let subscription = "projects/example/subscriptions/imported-a7d9-sub";
            let original = if cloud_event {
                "imported-a7d9"
            } else {
                subscription
            };
            for registration in [original, "jobs", "*"] {
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
                    collector: HashMap::new(),
                    control_server: control.clone(),
                    app_http_port: None,
                    command_timeout: Duration::from_secs(30),
                    http_client: create_forward_client(),
                    proxy_shutdown: CancellationToken::new(),
                };
                let body = serde_json::json!({"message": {"data": "e30=", "messageId": "provider-id"}, "subscription": subscription});
                let mut request = Request::builder()
                    .method("POST")
                    .uri("/__alien/queue/jobs")
                    .header("content-type", "application/json");
                if cloud_event {
                    request = request
                        .header("ce-specversion", "1.0")
                        .header("ce-id", "event-id")
                        .header("ce-time", "2026-01-01T00:00:00Z")
                        .header("ce-type", "google.cloud.pubsub.topic.v1.messagePublished")
                        .header(
                            "ce-source",
                            "//pubsub.googleapis.com/projects/example/topics/generated-jobs",
                        );
                }
                let request = request.body(Body::from(body.to_string())).expect("request");
                let response = tokio::spawn(async move {
                    handle_request(State(state), request).await.into_response()
                });
                let task = tokio::time::timeout(Duration::from_secs(2), tasks.next())
                    .await
                    .expect("dispatch")
                    .expect("stream")
                    .expect("task");
                let Some(Payload::QueueMessage(message)) = task.payload else {
                    panic!("single expected")
                };
                assert_eq!(
                    message.source,
                    if registration == "jobs" {
                        "jobs"
                    } else {
                        original
                    }
                );
                assert_eq!(message.id, "provider-id");
                assert!(!response.is_finished());
                control
                    .send_task_result(tonic::Request::new(SendTaskResultRequest {
                        task_id: task.task_id,
                        result: Some(send_task_result_request::Result::Success(
                            alien_worker_protocol::control::TaskSuccess {
                                response_data: Vec::new(),
                            },
                        )),
                    }))
                    .await
                    .expect("result");
                assert!(response.await.expect("response").status().is_success());
            }
        }
    }

    #[tokio::test]
    async fn physical_pubsub_names_dispatch_logical_batches_and_wait_for_outcomes() {
        // Imported subscriptions need not share any naming convention with the
        // logical queue or with their topic. Exercise both push wire formats.
        for cloud_event in [false, true] {
            for path in ["/", "/__alien/queue/jobs"] {
                let control = Arc::new(ControlGrpcServer::new());
                let mut tasks = control
                    .wait_for_tasks(tonic::Request::new(WaitForTasksRequest {
                        application_id: "worker".into(),
                    }))
                    .await
                    .expect("subscribe")
                    .into_inner();
                let subscription = "projects/example/subscriptions/imported-a7d9-sub";
                let environment = HashMap::from([
                    ("ALIEN_QUEUE_BATCH_CONFIG".into(), r#"{"jobs":{"maxMessages":1,"maxWindowSeconds":0}}"#.into()),
                    ("ALIEN_JOBS_BINDING".into(), serde_json::json!({
                        "service": "pubsub", "topic": "projects/example/topics/generated-b8c2-jobs",
                        "subscription": subscription,
                    }).to_string()),
                ]);
                let collectors = super::super::queue_batch::QueueCollectors::from_environment(
                    &environment,
                    control.clone(),
                )
                .expect("source bindings");
                let state = TransportState {
                    received_at: tokio::time::Instant::now(),
                    collector: collectors.clients.clone(),
                    control_server: control.clone(),
                    app_http_port: None,
                    command_timeout: Duration::from_secs(30),
                    http_client: create_forward_client(),
                    proxy_shutdown: CancellationToken::new(),
                };
                let body = serde_json::json!({"message": {"data": "e30=", "messageId": "provider-id"}, "subscription": subscription});
                let mut request = Request::builder()
                    .method("POST")
                    .uri(path)
                    .header("content-type", "application/json");
                if cloud_event {
                    request = request
                        .header("ce-specversion", "1.0")
                        .header("ce-id", "event-id")
                        .header("ce-type", "google.cloud.pubsub.topic.v1.messagePublished")
                        .header(
                            "ce-source",
                            "//pubsub.googleapis.com/projects/example/topics/generated-b8c2-jobs",
                        )
                        .header("ce-time", "2026-01-01T00:00:00Z");
                }
                let request = request.body(Body::from(body.to_string())).expect("request");
                let response = tokio::spawn(async move {
                    handle_request(State(state), request).await.into_response()
                });
                let task = tokio::time::timeout(Duration::from_secs(2), tasks.next())
                    .await
                    .expect("batch dispatched")
                    .expect("stream")
                    .expect("task");
                let Some(Payload::QueueBatch(batch)) = task.payload else {
                    panic!("configured source must dispatch a batch");
                };
                assert_eq!(batch.source, "jobs");
                assert_eq!(batch.deliveries.len(), 1);
                assert_eq!(
                    batch.deliveries[0]
                        .message
                        .as_ref()
                        .expect("message")
                        .source,
                    "jobs"
                );
                assert!(!response.is_finished(), "enqueue must not acknowledge");
                control
                    .send_task_result(tonic::Request::new(SendTaskResultRequest {
                        task_id: task.task_id,
                        result: Some(send_task_result_request::Result::QueueBatch(
                            QueueBatchResult {
                                outcomes: vec![QueueDeliveryOutcome {
                                    delivery_id: batch.deliveries[0].delivery_id.clone(),
                                    disposition: Disposition::Success as i32,
                                }],
                            },
                        )),
                    }))
                    .await
                    .expect("result");
                assert!(response
                    .await
                    .expect("request completes")
                    .status()
                    .is_success());
                collectors.join().await.expect("owned collectors stop");
            }
        }
    }
    #[tokio::test]
    async fn storage_notifications_remain_batched_queue_work() {
        for cloud_event in [false, true] {
            let control = Arc::new(ControlGrpcServer::new());
            let mut tasks = control
                .wait_for_tasks(tonic::Request::new(WaitForTasksRequest {
                    application_id: "worker".into(),
                }))
                .await
                .unwrap()
                .into_inner();
            let environment = std::collections::HashMap::from([
                ("ALIEN_QUEUE_BATCH_CONFIG".into(), r#"{"jobs":{"maxMessages":2,"maxWindowSeconds":5}}"#.into()),
                ("ALIEN_JOBS_BINDING".into(), serde_json::json!({"service":"pubsub","topic":"projects/example/topics/jobs","subscription":"projects/example/subscriptions/jobs-sub"}).to_string()),
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
                let object =
                    serde_json::json!({"bucket":"source","name":key,"size":"7","etag":"etag"});
                let body = serde_json::json!({"message": {"messageId":key,"data":general_purpose::STANDARD.encode(object.to_string()), "attributes": {
                    "notificationConfig":"projects/_/buckets/source/notificationConfigs/1", "storageNotificationRule":"source/0", "eventType":"OBJECT_FINALIZE", "payloadFormat":"JSON_API_V1", "eventTime":"2026-10-30T08:00:00Z", "storageEventSuffixes0":"[\".json\"]"
                }}, "subscription":"projects/example/subscriptions/jobs-sub"});
                let mut builder = Request::builder()
                    .method("POST")
                    .uri("/__alien/queue/jobs")
                    .header("content-type", "application/json");
                if cloud_event {
                    builder = builder
                        .header("ce-specversion", "1.0")
                        .header("ce-id", key)
                        .header("ce-type", "google.cloud.pubsub.topic.v1.messagePublished")
                        .header(
                            "ce-source",
                            "//pubsub.googleapis.com/projects/example/topics/jobs",
                        )
                        .header("ce-time", "2026-10-30T08:00:00Z");
                }
                builder.body(Body::from(body.to_string())).unwrap()
            };
            let excluded = handle_request(State(state.clone()), request("in/excluded.txt"))
                .await
                .into_response();
            assert_eq!(excluded.status(), StatusCode::OK);
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
