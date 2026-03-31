//! Shared utilities for HTTP-based transports.
//!
//! Contains common code used by CloudRun, ContainerApp, and Local transports:
//! - HTTP request forwarding to application
//! - Commands envelope parsing and response submission
//! - CloudEvents parsing from HTTP headers

use alien_bindings::control::ArcCommand;
use alien_commands::Envelope;
use axum::{
    body::{Body, Bytes},
    http::{header, Request, Response, StatusCode},
    response::IntoResponse,
};
use chrono::Utc;
use cloudevents::EventBuilder;
use futures_util::TryStreamExt;
use http_body_util::BodyExt;
use prost_types::Timestamp;
use tracing::error;

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

/// Convert an Envelope into an ArcCommand, fetching storage params if needed.
pub async fn envelope_to_command(envelope: &Envelope) -> Option<ArcCommand> {
    let params_bytes = alien_commands::runtime::decode_params_bytes(envelope)
        .await
        .ok()?;

    Some(ArcCommand {
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

/// Try to parse a queue message as a command envelope (legacy sync API).
///
/// WARNING: Returns empty params for storage mode. Prefer `try_parse_envelope`
/// + `envelope_to_command` for proper storage param support.
pub fn try_parse_arc_envelope(qm: &alien_core::QueueMessage) -> Option<ArcCommand> {
    let envelope = try_parse_envelope(qm)?;

    let params_bytes = match &envelope.params {
        alien_commands::BodySpec::Inline { inline_base64 } => {
            use base64::{engine::general_purpose, Engine as _};
            general_purpose::STANDARD.decode(inline_base64).ok()?
        }
        alien_commands::BodySpec::Storage {
            storage_get_request,
            ..
        } => {
            if storage_get_request.is_some() {
                vec![]
            } else {
                return None;
            }
        }
    };

    Some(ArcCommand {
        command_id: envelope.command_id,
        command_name: envelope.command,
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
        response_url: envelope.response_handling.submit_response_url,
    })
}


/// Parse CloudEvent from HTTP headers and body.
///
/// Supports both structured format (JSON body) and binary format (headers + body).
pub fn parse_cloudevent_from_http(
    headers: &axum::http::HeaderMap,
    body: &Bytes,
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

/// Parse CloudEvent with Dapr extension headers.
///
/// Same as `parse_cloudevent_from_http` but also extracts Dapr-specific extensions.
pub fn parse_cloudevent_from_http_with_extensions(
    headers: &axum::http::HeaderMap,
    body: &Bytes,
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
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if name_str.starts_with("ce-")
            && !["ce-id", "ce-source", "ce-type", "ce-time", "ce-specversion"].contains(&name_str)
        {
            if let Ok(v) = value.to_str() {
                let ext_name = name_str.trim_start_matches("ce-");
                builder = builder.extension(ext_name, v.to_string());
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
