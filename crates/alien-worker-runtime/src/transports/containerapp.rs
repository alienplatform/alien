//! Container App Transport
//!
//! Receives work via HTTP with Dapr integration:
//! - HTTP requests → forwarded to app's HTTP server
//! - Blob CloudEvent → StorageEvent via gRPC
//! - Service Bus (Dapr) → QueueMessage via gRPC (or Command if command envelope)
//! - Timer trigger → CronEvent via gRPC

use std::net::SocketAddr;
use std::sync::Arc;

use alien_worker_protocol::ControlGrpcServer;
use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{Request, Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use chrono::Utc;
use cloudevents::AttributesReader;
use http_body_util::BodyExt;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

use super::shared::{
    create_forward_client, dispatch_queue_messages, envelope_to_command, forward_http_request,
    handle_command, parse_cloudevent_from_http_with_extensions, send_cron_event,
    send_queue_message, send_storage_events, submit_decode_error,
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
    shutdown_rx: broadcast::Receiver<()>,
}

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

        info!(port = self.port, "Starting Container App transport");

        let state = TransportState {
            control_server: self.control_server,
            app_http_port: self.app_http_port,
            http_client: create_forward_client(),
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

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                self.shutdown_rx.recv().await.ok();
                info!("Container App transport received shutdown signal");
            })
            .await
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
    control_server: Arc<ControlGrpcServer>,
    app_http_port: Option<u16>,
    http_client: reqwest::Client,
}

async fn handle_request(
    State(state): State<TransportState>,
    request: Request<Body>,
) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let method = request.method().clone();

    debug!(path = %path, method = %method, "Received request");

    // Check for Azure Timer trigger (Container Apps Jobs)
    let is_timer_trigger =
        request.headers().get("X-Azure-Timer").is_some() || path.starts_with("/api/timer");

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
    let is_dapr = request.headers().get("dapr-content-type").is_some()
        || path.contains("/pubsub/")
        || path.starts_with("/servicebus-")
        || path.starts_with("/blobstorage-")
        || ce_type.as_deref() == Some("com.dapr.event.sent");

    if is_timer_trigger {
        return handle_timer_trigger(request, &state).await;
    }

    if is_dapr {
        return handle_dapr_message(request, &state).await;
    }

    if let Some(event_type) = ce_type {
        return handle_cloudevent(request, &event_type, &state).await;
    }

    // Forward HTTP request to app
    if let Some(app_port) = state.app_http_port {
        return forward_http_request(&state.http_client, request, app_port).await;
    }

    error!("No app HTTP port registered");
    (StatusCode::SERVICE_UNAVAILABLE, "No application registered").into_response()
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

    send_cron_event(schedule_name, schedule_time, &state.control_server).await
}

/// Handle Dapr messages (input bindings and pub/sub from Service Bus)
async fn handle_dapr_message(request: Request<Body>, state: &TransportState) -> Response<Body> {
    debug!("Processing Dapr message");

    if request.uri().path().starts_with("/blobstorage-") {
        return handle_dapr_storage_message(request, state).await;
    }

    // Collect body for CloudEvent parsing
    let (parts, body) = request.into_parts();
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
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
            return handle_raw_dapr_message(&body_bytes, state).await;
        }
    };

    // Process as Dapr CloudEvent (Service Bus message)
    match dapr_cloudevent_to_queue_messages(cloud_event) {
        Ok(queue_messages) => dispatch_queue_messages(queue_messages, &state.control_server).await,
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
    let body_bytes = match request.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
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
        let response = send_storage_events(storage_events, &state.control_server).await;
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
async fn handle_raw_dapr_message(body_bytes: &Bytes, state: &TransportState) -> Response<Body> {
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
            match envelope_to_command(&envelope).await {
                Ok(command) => {
                    if let Err(e) = handle_command(&envelope, &command, &state.control_server).await
                    {
                        error!(error = %e, "Failed to handle command");
                    }
                    return StatusCode::OK.into_response();
                }
                Err(e) => {
                    error!(command_id = %envelope.command_id, error = %e, "Failed to decode command params");
                    submit_decode_error(&envelope, &e).await;
                    return (StatusCode::BAD_REQUEST, "Failed to decode command").into_response();
                }
            }
        }
    }

    // Not a command — treat as regular queue message
    let message_id = json_value
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let source = json_value
        .get("topic")
        .or_else(|| json_value.get("pubsubname"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let qm = alien_core::QueueMessage {
        id: message_id,
        payload: alien_core::MessagePayload::Json(json_value),
        receipt_handle: String::new(),
        timestamp: Utc::now(),
        source,
        attributes: std::collections::HashMap::new(),
        attempt_count: None,
    };

    if let Err(e) = send_queue_message(&qm, &state.control_server).await {
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
    let body_bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
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
        Ok(storage_events) => send_storage_events(storage_events, &state.control_server).await,
        Err(e) => {
            error!(error = %e, event_type = %event_type, "Failed to parse Azure storage CloudEvent");
            (StatusCode::BAD_REQUEST, "Invalid storage event").into_response()
        }
    }
}
