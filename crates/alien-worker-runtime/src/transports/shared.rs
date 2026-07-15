//! Shared utilities for HTTP-based transports.
//!
//! Contains common code used by CloudRun, ContainerApp, and Local transports:
//! - HTTP request forwarding to application
//! - Commands envelope parsing and response submission
//! - CloudEvents parsing from HTTP headers

use std::{future::Future, sync::Arc, time::Duration};

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
    Router,
};
use chrono::{DateTime, Utc};
use cloudevents::EventBuilder;
use futures_util::{StreamExt, TryStreamExt};
use http_body_util::BodyExt;
use prost_types::Timestamp;
use tokio::{net::TcpListener, sync::broadcast, sync::Semaphore, time::Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

fn http_server_join_result(
    joined: std::result::Result<std::io::Result<()>, tokio::task::JoinError>,
) -> std::io::Result<()> {
    match joined {
        Ok(result) => result,
        Err(error) => Err(std::io::Error::other(error)),
    }
}

const FORCED_HTTP_SHUTDOWN_WAIT: Duration = Duration::from_secs(1);

/// Serve one Axum transport and bound graceful shutdown even when a proxied
/// response stream never ends. `on_shutdown` lets a transport stop admitting
/// work before Axum begins draining its already-accepted requests.
pub(super) async fn serve_with_bounded_shutdown(
    listener: TcpListener,
    app: Router,
    mut shutdown_rx: broadcast::Receiver<()>,
    proxy_shutdown: CancellationToken,
    shutdown_grace: Duration,
    transport_name: &'static str,
    on_shutdown: impl FnOnce(),
) -> std::io::Result<()> {
    let (graceful_tx, graceful_rx) = tokio::sync::oneshot::channel::<()>();
    let mut server_task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = graceful_rx.await;
            })
            .await
    });

    tokio::select! {
        joined = &mut server_task => http_server_join_result(joined),
        _ = shutdown_rx.recv() => {
            on_shutdown();
            tracing::info!(transport = transport_name, "HTTP transport received shutdown signal");
            let _ = graceful_tx.send(());
            match tokio::time::timeout(shutdown_grace, &mut server_task).await {
                Ok(joined) => http_server_join_result(joined),
                Err(_) => {
                    warn!(
                        transport = transport_name,
                        grace_seconds = shutdown_grace.as_secs_f64(),
                        "Active HTTP requests exceeded shutdown grace; closing proxy streams"
                    );
                    proxy_shutdown.cancel();
                    match tokio::time::timeout(FORCED_HTTP_SHUTDOWN_WAIT, &mut server_task).await {
                        Ok(joined) => http_server_join_result(joined),
                        Err(_) => {
                            server_task.abort();
                            let _ = server_task.await;
                            Ok(())
                        }
                    }
                }
            }
        }
    }
}

/// Timeout for event (queue/storage/cron) `send_task` round-trips.
pub(crate) const EVENT_TASK_TIMEOUT: Duration = Duration::from_secs(300);

/// Bound one pushed command by both its configured Worker timeout and the
/// envelope deadline. `None` means the deadline has already elapsed and the
/// application must not receive the command.
fn command_task_timeout(
    configured_timeout: Duration,
    deadline: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<Duration> {
    let deadline_timeout = match deadline {
        Some(deadline) => (deadline - now).to_std().ok(),
        None => Some(configured_timeout),
    }?;
    let timeout = configured_timeout.min(deadline_timeout);
    (!timeout.is_zero()).then_some(timeout)
}

/// Absolute budget for one pushed command, measured from HTTP receipt.
///
/// The same deadline covers the runtime queue, storage-backed params decode,
/// and application execution. Response submission has its own 30-second HTTP
/// timeout and fits inside the operator's additional 60-second lease headroom.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CommandBudget {
    deadline: Instant,
}

impl CommandBudget {
    pub(crate) fn from_envelope(
        configured_timeout: Duration,
        deadline: Option<DateTime<Utc>>,
    ) -> Option<Self> {
        Self::from_times(configured_timeout, deadline, Utc::now(), Instant::now())
    }

    fn from_times(
        configured_timeout: Duration,
        deadline: Option<DateTime<Utc>>,
        now_utc: DateTime<Utc>,
        now: Instant,
    ) -> Option<Self> {
        command_task_timeout(configured_timeout, deadline, now_utc).map(|timeout| Self {
            deadline: now + timeout,
        })
    }

    fn remaining(self) -> Option<Duration> {
        self.deadline
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
    }

    async fn run<F>(self, future: F) -> std::result::Result<F::Output, ()>
    where
        F: Future,
    {
        let Some(remaining) = self.remaining() else {
            return Err(());
        };
        tokio::time::timeout(remaining, future)
            .await
            .map_err(|_| ())
    }
}

enum CommandPreparation {
    Ready(ArcCommand),
    DecodeFailed(alien_commands::Error),
    BudgetElapsed,
}

/// Decode a pushed command under the same absolute budget used for queueing
/// and application execution. A missing budget is checked before the decode
/// future is polled, so an already-expired storage command performs no GET.
async fn prepare_pushed_command(
    envelope: &Envelope,
    budget: Option<CommandBudget>,
) -> CommandPreparation {
    let Some(budget) = budget else {
        return CommandPreparation::BudgetElapsed;
    };

    match budget.run(envelope_to_command(envelope)).await {
        Ok(Ok(command)) => CommandPreparation::Ready(command),
        Ok(Err(error)) => CommandPreparation::DecodeFailed(error),
        Err(()) => CommandPreparation::BudgetElapsed,
    }
}

/// Process one HTTP-pushed command under a runtime-scoped concurrency permit.
/// Waiting for the permit consumes the command's absolute budget, so queued
/// commands cannot outlive their lease and later execute as duplicates.
pub(crate) async fn process_pushed_command(
    envelope: Envelope,
    control_server: Arc<ControlGrpcServer>,
    concurrency: Arc<Semaphore>,
    budget: Option<CommandBudget>,
) {
    let Some(budget) = budget else {
        submit_budget_error(&envelope, "before command processing began").await;
        return;
    };

    let permit = match budget.run(concurrency.acquire_owned()).await {
        Ok(Ok(permit)) => permit,
        Ok(Err(error)) => {
            error!(
                command_id = %envelope.command_id,
                error = %error,
                "Command concurrency gate closed unexpectedly"
            );
            submit_processing_error(&envelope, "Command concurrency gate is unavailable").await;
            return;
        }
        Err(()) => {
            submit_budget_error(&envelope, "while waiting for execution capacity").await;
            return;
        }
    };

    process_command_with_budget(&envelope, &control_server, Some(budget)).await;

    drop(permit);
}

/// Process a command received from a native platform push. The budget begins
/// before storage params are fetched, matching the Local/Kubernetes HTTP push
/// path. Returns `false` only for a params-decode error so transports that use
/// non-2xx responses for native delivery retries can preserve that behavior.
pub(crate) async fn process_received_command(
    envelope: &Envelope,
    control_server: &ControlGrpcServer,
    configured_timeout: Duration,
) -> bool {
    let budget = CommandBudget::from_envelope(configured_timeout, envelope.deadline);
    process_command_with_budget(envelope, control_server, budget).await
}

async fn process_command_with_budget(
    envelope: &Envelope,
    control_server: &ControlGrpcServer,
    budget: Option<CommandBudget>,
) -> bool {
    match prepare_pushed_command(envelope, budget).await {
        CommandPreparation::Ready(command) => {
            if let Err(error) = handle_command(envelope, &command, control_server, budget).await {
                error!(
                    command_id = %envelope.command_id,
                    error = %error,
                    "Failed to process pushed command"
                );
            }
            true
        }
        CommandPreparation::DecodeFailed(error) => {
            error!(
                command_id = %envelope.command_id,
                error = %error,
                "Failed to decode pushed command params"
            );
            submit_decode_error(envelope, &error).await;
            false
        }
        CommandPreparation::BudgetElapsed => {
            submit_budget_error(envelope, "while decoding command params").await;
            true
        }
    }
}

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
    shutdown: CancellationToken,
) -> Response<Body> {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();

    // Build target URL
    let path_and_query = uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("/");
    let target_url = format!("http://127.0.0.1:{}{}", app_port, path_and_query);

    // Collect request body
    let body_bytes = match tokio::select! {
        body = request.into_body().collect() => body,
        _ = shutdown.cancelled() => {
            return (StatusCode::SERVICE_UNAVAILABLE, "Runtime is shutting down").into_response();
        }
    } {
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
    let forwarded = tokio::select! {
        response = req_builder.send() => response,
        _ = shutdown.cancelled() => {
            return (StatusCode::SERVICE_UNAVAILABLE, "Runtime is shutting down").into_response();
        }
    };

    match forwarded {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
            let resp_headers = resp.headers().clone();

            // Stream the response body instead of buffering it
            let byte_stream = resp
                .bytes_stream()
                .map_err(std::io::Error::other)
                .take_until(shutdown.cancelled_owned());
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
    budget: Option<CommandBudget>,
) -> std::result::Result<(), String> {
    let command_id = &command.command_id;
    let command_name = &command.command_name;

    tracing::info!(command_id = %command_id, command = %command_name, "Command received");

    let command_response = match budget.and_then(CommandBudget::remaining) {
        None => {
            warn!(
                command_id = %command_id,
                deadline = ?envelope.deadline,
                "Command deadline elapsed before execution; skipping application handler"
            );
            alien_commands::CommandResponse::error(
                "COMMAND_EXPIRED",
                format!("Command '{}' has expired", command.command_name),
            )
        }
        Some(timeout) => {
            let task = Task {
                task_id: command.command_id.clone(),
                payload: Some(control::task::Payload::ArcCommand(command.clone())),
            };

            debug!(
                command_id = %command_id,
                timeout_seconds = timeout.as_secs_f64(),
                "Sending command task to application via gRPC"
            );
            match control_server.send_task(task, timeout).await {
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
            }
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
    command_timeout: Duration,
) -> Response<Body> {
    for qm in queue_messages {
        if let Some(envelope) = try_parse_envelope(&qm) {
            process_received_command(&envelope, control_server, command_timeout).await;
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

async fn submit_budget_error(envelope: &Envelope, phase: &str) {
    warn!(
        command_id = %envelope.command_id,
        command = %envelope.command,
        phase,
        "Command budget elapsed; skipping remaining work"
    );
    let response = alien_commands::CommandResponse::error(
        "COMMAND_EXPIRED",
        format!("Command '{}' expired {phase}", envelope.command),
    );
    if let Err(error) = alien_commands::runtime::submit_response(envelope, response).await {
        error!(
            command_id = %envelope.command_id,
            error = %error,
            "Failed to submit command-expired response"
        );
    }
}

async fn submit_processing_error(envelope: &Envelope, message: &str) {
    let response = alien_commands::CommandResponse::error("PROCESSING_FAILED", message);
    if let Err(error) = alien_commands::runtime::submit_response(envelope, response).await {
        error!(
            command_id = %envelope.command_id,
            error = %error,
            "Failed to submit command-processing error response"
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

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use alien_core::presigned::{PresignedOperation, PresignedRequest};
    use axum::{routing::get, Router};
    use chrono::Duration as ChronoDuration;

    use super::*;

    #[test]
    fn command_timeout_supports_full_worker_window() {
        let now = Utc::now();

        assert_eq!(
            command_task_timeout(Duration::from_secs(3600), None, now),
            Some(Duration::from_secs(3600))
        );
    }

    #[test]
    fn command_timeout_uses_earlier_envelope_deadline() {
        let now = Utc::now();

        assert_eq!(
            command_task_timeout(
                Duration::from_secs(3600),
                Some(now + ChronoDuration::seconds(75)),
                now,
            ),
            Some(Duration::from_secs(75))
        );
    }

    #[test]
    fn expired_command_has_no_execution_budget() {
        let now = Utc::now();

        assert_eq!(
            command_task_timeout(
                Duration::from_secs(3600),
                Some(now - ChronoDuration::milliseconds(1)),
                now,
            ),
            None
        );
    }

    fn storage_envelope(url: String, deadline: Option<DateTime<Utc>>) -> Envelope {
        let mut envelope = alien_commands::test_utils::test_simple_envelope("command-id", "run");
        envelope.deadline = deadline;
        envelope.params = alien_commands::BodySpec::Storage {
            size: Some(2),
            storage_get_request: Some(PresignedRequest::new_http(
                url,
                "GET".to_string(),
                HashMap::new(),
                PresignedOperation::Get,
                "commands/command-id/params".to_string(),
                Utc::now() + ChronoDuration::minutes(5),
            )),
            storage_put_used: Some(true),
        };
        envelope
    }

    #[tokio::test]
    async fn expired_storage_command_never_fetches_params() {
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_in_handler = requests.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/params",
                    get(move || {
                        let requests = requests_in_handler.clone();
                        async move {
                            requests.fetch_add(1, Ordering::SeqCst);
                            "{}"
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let envelope = storage_envelope(
            format!("http://{address}/params"),
            Some(Utc::now() - ChronoDuration::seconds(1)),
        );
        let budget = CommandBudget::from_envelope(Duration::from_secs(60), envelope.deadline);

        let result = prepare_pushed_command(&envelope, budget).await;

        assert!(matches!(result, CommandPreparation::BudgetElapsed));
        assert_eq!(requests.load(Ordering::SeqCst), 0);
        server.abort();
    }

    #[tokio::test]
    async fn storage_params_get_is_bounded_by_the_total_command_budget() {
        let requests = Arc::new(AtomicUsize::new(0));
        let requests_in_handler = requests.clone();
        let started = Arc::new(tokio::sync::Notify::new());
        let started_in_handler = started.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new().route(
                    "/params",
                    get(move || {
                        let requests = requests_in_handler.clone();
                        let started = started_in_handler.clone();
                        async move {
                            requests.fetch_add(1, Ordering::SeqCst);
                            started.notify_one();
                            std::future::pending::<&'static str>().await
                        }
                    }),
                ),
            )
            .await
            .unwrap();
        });
        let envelope = storage_envelope(format!("http://{address}/params"), None);
        // Keep enough headroom for this network-backed test even when the full
        // suite is running many async tests concurrently. The pending response
        // still proves the one command budget terminates the GET.
        let budget = CommandBudget::from_envelope(Duration::from_secs(2), None);
        let decode = tokio::spawn(async move { prepare_pushed_command(&envelope, budget).await });

        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .expect("params GET must begin before the command budget expires");

        let result = tokio::time::timeout(Duration::from_secs(5), decode)
            .await
            .expect("command budget must bound the params GET")
            .expect("decode task must join");

        assert!(matches!(result, CommandPreparation::BudgetElapsed));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
