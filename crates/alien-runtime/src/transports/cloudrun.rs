//! Cloud Run Transport
//!
//! Receives work via HTTP with CloudEvents:
//! - HTTP requests → forwarded to app's HTTP server
//! - GCS CloudEvent → StorageEvent via gRPC
//! - Pub/Sub CloudEvent → QueueMessage via gRPC (or ArcCommand if ARC envelope)
//! - Cloud Scheduler → CronEvent via gRPC

use std::net::SocketAddr;
use std::sync::Arc;

use alien_bindings::control::{
    self, ArcCommand, CronEvent, QueueMessage as ProtoQueueMessage, StorageEvent, Task,
};
use alien_bindings::grpc::control_service::ControlGrpcServer;
use axum::{
    body::Body,
    extract::State,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use chrono::Utc;
use cloudevents::AttributesReader;
use http_body_util::BodyExt;
use prost_types::Timestamp;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use super::shared::{
    forward_http_request, parse_cloudevent_from_http, submit_arc_response,
    submit_arc_response_direct, try_parse_arc_envelope,
};
use crate::error::{ErrorData, Result};
use crate::events::gcp::{
    pubsub_cloudevent_to_queue_messages, storage_cloudevent_to_storage_events,
};
use alien_error::{AlienError, Context, IntoAlienError};

/// Cloud Run transport
pub struct CloudRunTransport {
    port: u16,
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    shutdown_rx: broadcast::Receiver<()>,
}

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
            shutdown_rx,
        }
    }

    pub fn with_app_port(mut self, port: u16) -> Self {
        self.app_http_port = Some(port);
        self
    }

    /// Run the transport
    pub async fn run(mut self) -> Result<()> {
        let addr = SocketAddr::from(([0, 0, 0, 0], self.port));

        info!(port = self.port, "Starting Cloud Run transport");

        let state = TransportState {
            control_server: self.control_server,
            app_http_port: self.app_http_port,
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

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                self.shutdown_rx.recv().await.ok();
                info!("Cloud Run transport received shutdown signal");
            })
            .await
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
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
}

async fn handle_request(
    State(state): State<TransportState>,
    request: Request<Body>,
) -> impl IntoResponse {
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

    // Forward HTTP request to app
    if let Some(app_port) = state.app_http_port {
        return forward_http_request(request, app_port).await;
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

    let task = Task {
        task_id: uuid::Uuid::new_v4().to_string(),
        payload: Some(control::task::Payload::CronEvent(CronEvent {
            schedule_name: job_name.clone(),
            scheduled_time: Some(Timestamp {
                seconds: schedule_time.timestamp(),
                nanos: schedule_time.timestamp_subsec_nanos() as i32,
            }),
        })),
    };

    match state
        .control_server
        .send_task(task, std::time::Duration::from_secs(300))
        .await
    {
        Ok(result) => {
            if result.success {
                StatusCode::OK.into_response()
            } else {
                error!(
                    error_code = ?result.error_code,
                    error_message = ?result.error_message,
                    "Application failed to process cron event"
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Application failed to process event",
                )
                    .into_response()
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to send cron event to application");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to communicate with application",
            )
                .into_response()
        }
    }
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
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
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
        return handle_pubsub_cloudevent(cloud_event, state).await;
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
            for se in storage_events.0 {
                let task = Task {
                    task_id: uuid::Uuid::new_v4().to_string(),
                    payload: Some(control::task::Payload::StorageEvent(StorageEvent {
                        bucket: se.bucket_name,
                        key: se.object_key,
                        size: se.size.unwrap_or(0),
                        event_type: format!("{:?}", se.event_type),
                        content_type: se.content_type.unwrap_or_default(),
                        timestamp: Some(Timestamp {
                            seconds: se.timestamp.timestamp(),
                            nanos: se.timestamp.timestamp_subsec_nanos() as i32,
                        }),
                    })),
                };

                match state
                    .control_server
                    .send_task(task, std::time::Duration::from_secs(300))
                    .await
                {
                    Ok(result) => {
                        if !result.success {
                            error!(
                                error_code = ?result.error_code,
                                error_message = ?result.error_message,
                                "Application failed to process storage event"
                            );
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "Application failed to process event",
                            )
                                .into_response();
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to send storage event to application");
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Failed to communicate with application",
                        )
                            .into_response();
                    }
                }
            }
            StatusCode::OK.into_response()
        }
        Err(e) => {
            error!(error = %e, event_type = %event_type, "Failed to parse storage CloudEvent");
            (StatusCode::BAD_REQUEST, "Invalid storage event").into_response()
        }
    }
}

/// Handle Pub/Sub CloudEvents - may contain ARC envelope or regular queue message
async fn handle_pubsub_cloudevent(
    cloud_event: cloudevents::Event,
    state: &TransportState,
) -> Response<Body> {
    match pubsub_cloudevent_to_queue_messages(cloud_event) {
        Ok(queue_messages) => {
            for qm in queue_messages {
                // Check if this is an ARC envelope
                if let Some(arc_command) = try_parse_arc_envelope(&qm) {
                    if let Err(e) = handle_arc_command(&arc_command, state).await {
                        error!(error = %e, "Failed to handle ARC command");
                    }
                } else {
                    // Regular queue message
                    if let Err(e) = send_queue_message(&qm, state).await {
                        error!(error = %e, "Failed to send queue message");
                        return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to process event")
                            .into_response();
                    }
                }
            }
            StatusCode::OK.into_response()
        }
        Err(e) => {
            error!(error = %e, "Failed to parse Pub/Sub CloudEvent");
            (StatusCode::BAD_REQUEST, "Invalid Pub/Sub event").into_response()
        }
    }
}

/// Handle an ARC command
async fn handle_arc_command(
    arc_command: &ArcCommand,
    state: &TransportState,
) -> std::result::Result<(), String> {
    let task = Task {
        task_id: arc_command.command_id.clone(),
        payload: Some(control::task::Payload::ArcCommand(arc_command.clone())),
    };

    match state
        .control_server
        .send_task(task, std::time::Duration::from_secs(300))
        .await
    {
        Ok(result) => submit_arc_response(arc_command, result).await,
        Err(e) => {
            let error_response = alien_commands::CommandResponse::error(
                "PROCESSING_FAILED",
                format!("Command processing failed: {}", e),
            );
            submit_arc_response_direct(arc_command, error_response).await
        }
    }
}

/// Send a queue message to the application
async fn send_queue_message(
    qm: &alien_core::QueueMessage,
    state: &TransportState,
) -> std::result::Result<(), String> {
    let payload_bytes = match &qm.payload {
        alien_core::MessagePayload::Json(v) => v.to_string().into_bytes(),
        alien_core::MessagePayload::Text(s) => s.clone().into_bytes(),
    };

    let task = Task {
        task_id: qm.id.clone(),
        payload: Some(control::task::Payload::QueueMessage(ProtoQueueMessage {
            id: qm.id.clone(),
            source: qm.source.clone(),
            payload: payload_bytes,
            receipt_handle: qm.receipt_handle.clone(),
            attempt_count: qm.attempt_count.unwrap_or(1),
            timestamp: Some(Timestamp {
                seconds: qm.timestamp.timestamp(),
                nanos: qm.timestamp.timestamp_subsec_nanos() as i32,
            }),
        })),
    };

    match state
        .control_server
        .send_task(task, std::time::Duration::from_secs(300))
        .await
    {
        Ok(result) => {
            if result.success {
                Ok(())
            } else {
                Err(format!(
                    "Application failed to process queue message: {} - {}",
                    result.error_code.unwrap_or_else(|| "UNKNOWN".to_string()),
                    result
                        .error_message
                        .unwrap_or_else(|| "No error message".to_string())
                ))
            }
        }
        Err(e) => Err(format!("Failed to send queue message: {}", e)),
    }
}
