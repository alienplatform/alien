//! Alien Test App - Minimal test application for runtime testing.
//!
//! This application is used for testing the alien-runtime, alien-local, and dockdash crates.
//! It provides minimal functionality needed for core runtime testing:
//! - HTTP server with health and inspect endpoints
//! - Event handlers for storage and queue events
//! - ARC commands for testing command invocation
//! - Minimal bindings usage (Storage, KV)

use alien_bindings::{AlienContext, ErrorData as BindingsErrorData};
use alien_error::{Context, IntoAlienError};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, Sse},
    routing::{get, post},
    Json, Router,
};
use clap::Parser;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::convert::Infallible;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio_stream::Stream;
use tracing::{error, info, warn};

/// Application state shared across handlers
#[derive(Clone)]
struct AppState {
    ctx: Arc<AlienContext>,
}

/// Command line arguments
#[derive(Parser, Debug)]
struct Args {
    /// Host address to bind to
    #[clap(long, value_parser, default_value = "127.0.0.1")]
    host: String,
}

/// Health check response
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: String,
    timestamp: String,
}

/// Inspect response
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InspectResponse {
    success: bool,
    request_body: serde_json::Value,
    timestamp: String,
}

#[tokio::main]
async fn main() {
    // Initialize logging
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info,alien_test_app=debug");
    }
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Read port from PORT environment variable (set by alien-runtime)
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);

    info!(
        port = port,
        source = if std::env::var("PORT").is_ok() {
            "PORT env var"
        } else {
            "dynamic"
        },
        "Starting Alien Test App"
    );

    // Initialize Alien context
    let ctx = AlienContext::from_env()
        .await
        .expect("Failed to create Alien context");

    info!(
        app_id = %ctx.application_id(),
        "Initialized Alien context"
    );

    let app_state = AppState { ctx: Arc::new(ctx) };

    // Register event handlers
    register_event_handlers(&app_state);

    // Build the HTTP router
    let app = build_router(app_state.clone());

    // Bind to address
    let addr = SocketAddr::new(
        args.host.parse::<IpAddr>().unwrap_or_else(|_| {
            warn!("Invalid host address '{}', using 127.0.0.1", args.host);
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
        }),
        port,
    );

    info!(addr = %addr, "Binding HTTP server");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind TCP listener");
    let actual_addr = listener.local_addr().expect("Failed to get local address");

    // Register HTTP server port with the runtime
    if let Err(e) = app_state.ctx.register_http_server(actual_addr.port()).await {
        warn!(
            error = %e,
            "Failed to register HTTP server with runtime (OK in local dev)"
        );
    } else {
        info!(
            port = actual_addr.port(),
            "Registered HTTP server with runtime"
        );
    }

    info!(addr = %actual_addr, "Server ready");

    // Start the event loop in a background task
    let ctx_for_events = app_state.ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = ctx_for_events.run().await {
            error!(error = %e, "Event loop error");
        }
    });

    // Run the HTTP server
    axum::serve(listener, app)
        .await
        .expect("Failed to run HTTP server");
}

/// Register event handlers with the Alien context
fn register_event_handlers(app_state: &AppState) {
    let ctx = app_state.ctx.clone();

    // Storage event handler - stores received events in KV for verification
    {
        let ctx_for_handler = ctx.clone();
        ctx.on_storage_event("*", move |event| {
            let ctx = ctx_for_handler.clone();
            async move {
                info!(
                    key = %event.key,
                    bucket = %event.bucket,
                    event_type = %event.event_type,
                    "Received storage event"
                );

                // Store in KV for test verification
                let kv = ctx.get_bindings().load_kv("test-kv").await?;
                let record = serde_json::json!({
                    "key": event.key,
                    "bucket": event.bucket,
                    "eventType": event.event_type,
                    "size": event.size,
                    "processedAt": chrono::Utc::now().to_rfc3339(),
                });
                let sanitized_key = event.key.replace('/', "_");
                let kv_key = format!("storage_event:{}", sanitized_key);
                let value = serde_json::to_vec(&record).into_alien_error().context(
                    BindingsErrorData::SerializationFailed {
                        message: "Failed to serialize storage event".to_string(),
                    },
                )?;
                kv.put(&kv_key, value, None).await?;

                Ok(())
            }
        });
    }

    // Queue message handler - stores received messages in KV for verification
    {
        let ctx_for_handler = ctx.clone();
        ctx.on_queue_message("*", move |message| {
            let ctx = ctx_for_handler.clone();
            async move {
                info!(
                    id = %message.id,
                    source = %message.source,
                    attempt = message.attempt_count,
                    "Received queue message"
                );

                // Store in KV for test verification
                let kv = ctx.get_bindings().load_kv("test-kv").await?;
                let record = serde_json::json!({
                    "messageId": message.id,
                    "source": message.source,
                    "payload": String::from_utf8_lossy(&message.payload).to_string(),
                    "attemptCount": message.attempt_count,
                    "processedAt": chrono::Utc::now().to_rfc3339(),
                });
                let sanitized_id = message.id.replace('/', "_");
                let kv_key = format!("queue_message:{}", sanitized_id);
                let value = serde_json::to_vec(&record).into_alien_error().context(
                    BindingsErrorData::SerializationFailed {
                        message: "Failed to serialize queue message".to_string(),
                    },
                )?;
                kv.put(&kv_key, value, None).await?;

                Ok(())
            }
        });
    }

    // ARC test command for small payloads (inline response)
    ctx.on_command("arc-test-small", |params: serde_json::Value| async move {
        info!(params = ?params, "Received arc-test-small command");

        let params_json = serde_json::to_string(&params).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(params_json.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        Ok(serde_json::json!({
            "success": true,
            "testType": "arc-small-payload",
            "paramsHash": hash,
            "paramsSize": params_json.len(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }))
    });

    // ARC test command for large payloads (storage-based response)
    ctx.on_command("arc-test-large", |params: serde_json::Value| async move {
        info!(params = ?params, "Received arc-test-large command");

        let params_json = serde_json::to_string(&params).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(params_json.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        // Generate large response (>48KB) to trigger storage mode
        let large_data = vec!["test-data-chunk"; 15000].join(" ");
        let bulk_array: Vec<String> = (0..8000).map(|i| format!("bulk-item-{}", i)).collect();

        Ok(serde_json::json!({
            "success": true,
            "testType": "arc-large-payload",
            "paramsHash": hash,
            "paramsSize": params_json.len(),
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "largeResponseData": large_data,
            "bulkData": bulk_array,
        }))
    });

    info!("Registered event handlers and commands");
}

/// Build the Axum router with HTTP endpoints
fn build_router(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/inspect", post(inspect_request))
        .route("/env-var/{*name}", get(get_env_var))
        .route("/sse", get(sse_stream))
        .route("/events/storage/{*key}", get(get_storage_event))
        .route("/events/queue/{*message_id}", get(get_queue_message))
        .with_state(app_state)
}

/// Health check endpoint
async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Inspect request endpoint - echoes back the received data
async fn inspect_request(Json(payload): Json<serde_json::Value>) -> Json<InspectResponse> {
    Json(InspectResponse {
        success: true,
        request_body: payload,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

/// Returns the value of an environment variable (for test verification)
async fn get_env_var(
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match std::env::var(&name) {
        Ok(value) => Ok(Json(serde_json::json!({
            "success": true,
            "variable": name,
            "value": value,
        }))),
        Err(_) => Err((
            StatusCode::NOT_FOUND,
            format!("Env var not found: {}", name),
        )),
    }
}

/// Simple SSE endpoint for transport testing
async fn sse_stream() -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let events = (0..10).map(|i| Ok(Event::default().data(format!("sse_message_{}", i))));

    Sse::new(tokio_stream::iter(events))
}

/// Get storage event from KV (for test verification)
async fn get_storage_event(
    State(app_state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let kv = app_state
        .ctx
        .get_bindings()
        .load_kv("test-kv")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let sanitized_key = key.replace('/', "_");
    let kv_key = format!("storage_event:{}", sanitized_key);

    match kv.get(&kv_key).await {
        Ok(Some(data)) => {
            let event: serde_json::Value = serde_json::from_slice(&data)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Ok(Json(event))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            format!("Storage event not found: {}", key),
        )),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}

/// Get queue message from KV (for test verification)
async fn get_queue_message(
    State(app_state): State<AppState>,
    Path(message_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let kv = app_state
        .ctx
        .get_bindings()
        .load_kv("test-kv")
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let sanitized_id = message_id.replace('/', "_");
    let kv_key = format!("queue_message:{}", sanitized_id);

    match kv.get(&kv_key).await {
        Ok(Some(data)) => {
            let message: serde_json::Value = serde_json::from_slice(&data)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Ok(Json(message))
        }
        Ok(None) => Err((
            StatusCode::NOT_FOUND,
            format!("Queue message not found: {}", message_id),
        )),
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())),
    }
}
