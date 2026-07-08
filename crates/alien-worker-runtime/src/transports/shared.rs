//! Shared utilities for HTTP-based transports.
//!
//! Contains common code used by CloudRun, ContainerApp, and Local transports:
//! - HTTP request forwarding to application
//! - Commands envelope parsing and response submission
//! - CloudEvents parsing from HTTP headers

use std::time::Duration;

use alien_commands::Envelope;
use alien_worker_protocol::{
    control::{
        self, ArcCommand, CronEvent, QueueMessage as ProtoQueueMessage,
        StorageEvent as ProtoStorageEvent, Task,
    },
    ControlGrpcServer,
};
use axum::{
    body::{Body, Bytes},
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
};
use chrono::{DateTime, Utc};
use cloudevents::EventBuilder;
use futures_util::TryStreamExt;
use http_body_util::BodyExt;
use prost_types::Timestamp;
use tracing::{debug, error};

/// Timeout for one command round-trip through the app (`send_task`).
///
/// 300 seconds, matching the queue/storage/cron `send_task` timeout and the
/// forward client's read timeout. The Cloud Run transport previously used
/// 120s — a value inherited from the Lambda transport, where the 180s
/// function timeout forces headroom to submit an error response before the
/// platform kills the invocation. Cloud Run and Container Apps have no such
/// cap, so commands get the full task window; the transport still bounds the
/// wait, so a hung handler yields an error response instead of leasing
/// forever.
pub(crate) const COMMAND_TASK_TIMEOUT: Duration = Duration::from_secs(300);

/// Timeout for event (queue/storage/cron) `send_task` round-trips.
pub(crate) const EVENT_TASK_TIMEOUT: Duration = Duration::from_secs(300);

/// Create a shared reqwest client for forwarding HTTP requests.
///
/// This client is meant to be created once and reused across all requests
/// to benefit from connection pooling. Configured for localhost forwarding
/// with no proxy, generous timeouts (the app may make slow cloud API calls),
/// and disabled TCP user timeout (gVisor on Cloud Run Gen2 can prematurely
/// close idle connections with the default settings).
pub fn create_forward_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .read_timeout(std::time::Duration::from_secs(300))
        .no_proxy()
        .tcp_keepalive(None)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// Forward an HTTP request to the application.
///
/// This is the core proxy logic used by all HTTP-based transports.
/// Supports streaming responses (SSE, chunked transfer, etc.).
pub async fn forward_http_request(
    client: &reqwest::Client,
    request: Request<Body>,
    app_port: u16,
) -> Response<Body> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();

    // Build target URL
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let target_url = format!("http://127.0.0.1:{}{}", app_port, path_and_query);

    // Collect request body
    let body_bytes = match request.into_body().collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!(error = %e, "Failed to read request body");
            return (StatusCode::BAD_REQUEST, "Failed to read body").into_response();
        }
    };

    // Build reqwest request
    let mut req_builder = client.request(
        reqwest::Method::from_bytes(method.as_str().as_bytes()).unwrap_or(reqwest::Method::GET),
        &target_url,
    );

    // Copy headers
    for (name, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            req_builder = req_builder.header(name.as_str(), v);
        }
    }

    // Add body
    req_builder = req_builder.body(body_bytes.to_vec());

    // Send request and stream response
    match req_builder.send().await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let resp_headers = resp.headers().clone();

            // Stream the response body instead of buffering it
            let byte_stream = resp
                .bytes_stream()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
            let stream_body = Body::from_stream(byte_stream);

            let mut response = Response::builder().status(status);

            for (name, value) in resp_headers.iter() {
                response = response.header(name.as_str(), value.as_bytes());
            }

            response
                .body(stream_body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            let is_connect = e.is_connect();
            let is_timeout = e.is_timeout();
            let is_request = e.is_request();
            error!(
                error = %e,
                target_url = %target_url,
                is_connect,
                is_timeout,
                is_request,
                "Failed to forward request"
            );
            (StatusCode::BAD_GATEWAY, "Failed to forward request").into_response()
        }
    }
}

/// Try to parse a queue message as a command envelope (detection only).
///
/// Returns `Some(Envelope)` if the message contains a valid command envelope,
/// `None` otherwise. Does NOT decode params — use `envelope_to_command`
/// for async param decoding (handles both inline and storage modes).
pub fn try_parse_envelope(qm: &alien_core::QueueMessage) -> Option<Envelope> {
    let json_str = match &qm.payload {
        alien_core::MessagePayload::Json(v) => serde_json::to_string(v).ok()?,
        alien_core::MessagePayload::Text(s) => s.clone(),
    };

    let envelope: Envelope = serde_json::from_str(&json_str).ok()?;

    if envelope.protocol != alien_commands::PROTOCOL_VERSION {
        return None;
    }

    Some(envelope)
}

/// Handle a command: send it to the app over gRPC and submit the response
/// (success or error) back to the manager.
pub(crate) async fn handle_command(
    envelope: &Envelope,
    command: &ArcCommand,
    control_server: &ControlGrpcServer,
) -> std::result::Result<(), String> {
    let command_id = &command.command_id;
    let command_name = &command.command_name;

    tracing::info!(command_id = %command_id, command = %command_name, "Command received");

    let task = Task {
        task_id: command.command_id.clone(),
        payload: Some(control::task::Payload::ArcCommand(command.clone())),
    };

    debug!(command_id = %command_id, "Sending command task to application via gRPC");
    let command_response = match control_server.send_task(task, COMMAND_TASK_TIMEOUT).await {
        Ok(result) => {
            debug!(
                command_id = %command_id,
                success = result.success,
                response_size = result.response_data.len(),
                "Received command result from application"
            );
            if result.success {
                alien_commands::CommandResponse::success(&result.response_data)
            } else {
                alien_commands::CommandResponse::error(
                    result.error_code.unwrap_or_else(|| "UNKNOWN".to_string()),
                    result
                        .error_message
                        .unwrap_or_else(|| "Unknown error".to_string()),
                )
            }
        }
        Err(e) => {
            error!(command_id = %command_id, error = %e, "Command task failed — send_task error");
            alien_commands::CommandResponse::error(
                "PROCESSING_FAILED",
                format!("Command processing failed: {}", e),
            )
        }
    };

    debug!(command_id = %command_id, "Submitting command response to manager");
    alien_commands::runtime::submit_response(envelope, command_response)
        .await
        .map_err(|e| {
            error!(command_id = %command_id, error = %e, "Failed to submit command response");
            format!("Failed to submit response: {}", e)
        })?;
    debug!(command_id = %command_id, "Command response submitted successfully");
    Ok(())
}

/// Send a queue message to the application.
pub(crate) async fn send_queue_message(
    qm: &alien_core::QueueMessage,
    control_server: &ControlGrpcServer,
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
            attributes: qm.attributes.clone(),
        })),
    };

    match control_server.send_task(task, EVENT_TASK_TIMEOUT).await {
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

/// Dispatch parsed queue messages to the app: command envelopes go through
/// the command path (decode params, send, submit response), everything else
/// is delivered as a regular queue message.
pub(crate) async fn dispatch_queue_messages(
    queue_messages: Vec<alien_core::QueueMessage>,
    control_server: &ControlGrpcServer,
) -> Response<Body> {
    for qm in queue_messages {
        if let Some(envelope) = try_parse_envelope(&qm) {
            match envelope_to_command(&envelope).await {
                Ok(command) => {
                    if let Err(e) = handle_command(&envelope, &command, control_server).await {
                        error!(error = %e, "Failed to handle command");
                    }
                }
                Err(e) => {
                    error!(command_id = %envelope.command_id, error = %e, "Failed to decode command params");
                    submit_decode_error(&envelope, &e).await;
                }
            }
        } else if let Err(e) = send_queue_message(&qm, control_server).await {
            error!(error = %e, "Failed to send queue message");
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to process event").into_response();
        }
    }
    StatusCode::OK.into_response()
}

/// Send converted storage events to the application, one task per event.
pub(crate) async fn send_storage_events(
    storage_events: alien_core::StorageEvents,
    control_server: &ControlGrpcServer,
) -> Response<Body> {
    for se in storage_events.0 {
        let task = Task {
            task_id: uuid::Uuid::new_v4().to_string(),
            payload: Some(control::task::Payload::StorageEvent(ProtoStorageEvent {
                bucket: se.bucket_name,
                key: se.object_key,
                size: se.size.unwrap_or(0),
                event_type: format!("{:?}", se.event_type),
                content_type: se.content_type.unwrap_or_default(),
                timestamp: Some(Timestamp {
                    seconds: se.timestamp.timestamp(),
                    nanos: se.timestamp.timestamp_subsec_nanos() as i32,
                }),
                etag: se.etag.unwrap_or_default(),
                region: se.region.unwrap_or_default(),
                version_id: se.version_id.unwrap_or_default(),
                current_tier: se.current_tier.unwrap_or_default(),
                metadata: se.metadata,
            })),
        };

        match control_server.send_task(task, EVENT_TASK_TIMEOUT).await {
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

/// Send a cron event to the application.
pub(crate) async fn send_cron_event(
    schedule_name: String,
    schedule_time: DateTime<Utc>,
    control_server: &ControlGrpcServer,
) -> Response<Body> {
    let task = Task {
        task_id: uuid::Uuid::new_v4().to_string(),
        payload: Some(control::task::Payload::CronEvent(CronEvent {
            schedule_name,
            scheduled_time: Some(Timestamp {
                seconds: schedule_time.timestamp(),
                nanos: schedule_time.timestamp_subsec_nanos() as i32,
            }),
        })),
    };

    match control_server.send_task(task, EVENT_TASK_TIMEOUT).await {
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

/// Submit a typed error response for a command whose params failed to decode,
/// under the decode error's own code — the same semantics as the pull receiver
/// and the Lambda transport. Best-effort: a submit failure is logged, not
/// propagated (there is nothing further to do with the command).
pub(crate) async fn submit_decode_error(envelope: &Envelope, error: &alien_commands::Error) {
    let response = alien_commands::CommandResponse::error(&error.code, error.to_string());
    if let Err(submit_err) = alien_commands::runtime::submit_response(envelope, response).await {
        error!(
            command_id = %envelope.command_id,
            error = %submit_err,
            "Failed to submit decode-error response"
        );
    }
}

/// Convert an Envelope into an ArcCommand, fetching storage params if needed.
///
/// Propagates the params-decode error (rather than swallowing it) so callers
/// can submit a typed error response under the decode error's own code — the
/// same semantics as the pull receiver and the Lambda transport.
pub async fn envelope_to_command(envelope: &Envelope) -> alien_commands::Result<ArcCommand> {
    let params_bytes = alien_commands::runtime::decode_params_bytes(envelope).await?;

    Ok(ArcCommand {
        command_id: envelope.command_id.clone(),
        command_name: envelope.command.clone(),
        params: params_bytes,
        attempt: envelope.attempt,
        deadline: envelope.deadline.map(|dt| Timestamp {
            seconds: dt.timestamp(),
            nanos: dt.timestamp_subsec_nanos() as i32,
        }),
        max_inline_bytes: envelope.response_handling.max_inline_bytes,
        storage_upload_url: envelope
            .response_handling
            .storage_upload_request
            .url()
            .to_string(),
        response_url: envelope.response_handling.submit_response_url.clone(),
    })
}

/// Parse CloudEvent from HTTP headers and body.
///
/// Supports both structured format (JSON body) and binary format (headers + body).
pub fn parse_cloudevent_from_http(
    headers: &axum::http::HeaderMap,
    body: &Bytes,
) -> std::result::Result<cloudevents::Event, String> {
    parse_cloudevent_from_http_impl(headers, body, false)
}

/// Parse CloudEvent with Dapr extension headers.
///
/// Same as `parse_cloudevent_from_http` but also extracts Dapr-specific extensions.
pub fn parse_cloudevent_from_http_with_extensions(
    headers: &axum::http::HeaderMap,
    body: &Bytes,
) -> std::result::Result<cloudevents::Event, String> {
    parse_cloudevent_from_http_impl(headers, body, true)
}

fn parse_cloudevent_from_http_impl(
    headers: &axum::http::HeaderMap,
    body: &Bytes,
    include_extensions: bool,
) -> std::result::Result<cloudevents::Event, String> {
    // Try structured format first (JSON body)
    if let Some(content_type) = headers.get(header::CONTENT_TYPE) {
        if content_type
            .to_str()
            .map(|s| s.contains("application/cloudevents+json"))
            .unwrap_or(false)
        {
            return serde_json::from_slice(body).map_err(|e| format!("JSON parse error: {}", e));
        }
    }

    // Try binary format (headers + body)
    let mut builder = cloudevents::EventBuilderV10::new();

    // Required attributes
    let id = headers
        .get("ce-id")
        .and_then(|v| v.to_str().ok())
        .ok_or("Missing ce-id header")?;
    let source = headers
        .get("ce-source")
        .and_then(|v| v.to_str().ok())
        .ok_or("Missing ce-source header")?;
    let ty = headers
        .get("ce-type")
        .and_then(|v| v.to_str().ok())
        .ok_or("Missing ce-type header")?;

    builder = builder.id(id).source(source).ty(ty);

    // Optional time
    if let Some(time) = headers.get("ce-time").and_then(|v| v.to_str().ok()) {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time) {
            builder = builder.time(dt.with_timezone(&Utc));
        }
    }

    // Extensions (Dapr-specific)
    if include_extensions {
        for (name, value) in headers.iter() {
            let name_str = name.as_str();
            if name_str.starts_with("ce-")
                && !["ce-id", "ce-source", "ce-type", "ce-time", "ce-specversion"]
                    .contains(&name_str)
            {
                if let Ok(v) = value.to_str() {
                    let ext_name = name_str.trim_start_matches("ce-");
                    builder = builder.extension(ext_name, v.to_string());
                }
            }
        }
    }

    // Data content type and data
    let data_content_type = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json");

    if data_content_type.contains("json") {
        let json_data: serde_json::Value =
            serde_json::from_slice(body).map_err(|e| format!("JSON data parse error: {}", e))?;
        builder = builder.data(data_content_type, json_data);
    } else {
        builder = builder.data(data_content_type, body.to_vec());
    }

    builder.build().map_err(|e| format!("Build error: {}", e))
}
