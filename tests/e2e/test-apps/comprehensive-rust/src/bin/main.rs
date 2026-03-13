//! Alien Test Server - Example application demonstrating the Alien SDK.
//!
//! This server showcases:
//! - HTTP server with the new `register_http_server` pattern
//! - Event handlers (storage, cron, queue, command)
//! - Background tasks with `wait_until`
//! - Bindings usage (storage, kv, queue, vault, etc.)

use alien_bindings::{AlienContext, ErrorData as BindingsErrorData};
use alien_error::{Context, IntoAlienError};
use alien_test_server::{handlers, models::AppState};
use axum::{
    routing::{get, post},
    Router,
};
use clap::Parser;
use sha2::{Digest, Sha256};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tracing::{error, info, warn};

/// Command line arguments for the test server.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Host address to bind to
    #[clap(long, value_parser, default_value = "127.0.0.1")]
    host: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "info,alien_test_server=debug");
    }
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    // Read port from PORT environment variable (set by alien-runtime), fallback to 0 (dynamic)
    // Using port 0 lets the OS pick a free port, which is safer for testing
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
        "Starting Alien Test Server"
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

    // Register event handlers (using new SDK pattern)
    // Note: These handlers are called via gRPC from the runtime when events occur
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

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let actual_addr = listener.local_addr()?;

    // Register HTTP server port with the runtime (new SDK pattern)
    // This tells the runtime where to forward HTTP requests
    if let Err(e) = app_state.ctx.register_http_server(actual_addr.port()).await {
        warn!(
            error = %e,
            "Failed to register HTTP server with runtime (this is OK in local development)"
        );
    } else {
        info!(
            port = actual_addr.port(),
            "Registered HTTP server with runtime"
        );
    }

    info!(addr = %actual_addr, "Server ready, accepting connections");

    // Start the event loop in a background task
    // This subscribes to WaitForEvents() and dispatches events to registered handlers
    let ctx_for_events = app_state.ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = ctx_for_events.run().await {
            error!(error = %e, "Event loop error");
        }
    });

    // Run the HTTP server
    axum::serve(listener, app).await?;

    Ok(())
}

/// Register event handlers with the Alien context.
/// These handlers are called by the runtime via gRPC when events occur.
/// Results are stored in KV for test verification.
fn register_event_handlers(app_state: &AppState) {
    let ctx = app_state.ctx.clone();

    // Storage event handler - stores received events in KV for test verification
    {
        let ctx_for_handler = ctx.clone();
        ctx.on_storage_event("*", move |event| {
            let ctx = ctx_for_handler.clone();
            async move {
                info!(
                    key = %event.key,
                    bucket = %event.bucket,
                    event_type = %event.event_type,
                    size = event.size,
                    "Received storage event"
                );

                // Store in KV for test verification
                // Sanitize key: replace / with _ to comply with KV key validation rules
                let kv = ctx.get_bindings().load_kv("test-alien-kv").await?;
                let record = serde_json::json!({
                    "key": event.key,
                    "bucket": event.bucket,
                    "eventType": event.event_type,
                    "size": event.size,
                    "contentType": event.content_type,
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
                info!(kv_key = %kv_key, "Stored storage event in KV");

                Ok(())
            }
        });
    }

    // Cron event handler - stores received events in KV for test verification
    {
        let ctx_for_handler = ctx.clone();
        ctx.on_cron_event("*", move |event| {
            let ctx = ctx_for_handler.clone();
            async move {
                info!(
                    schedule = %event.schedule_name,
                    scheduled_time = %event.scheduled_time,
                    "Received cron event"
                );

                // Store in KV for test verification
                // Sanitize schedule name: replace / with _ to comply with KV key validation rules
                let kv = ctx.get_bindings().load_kv("test-alien-kv").await?;
                let record = serde_json::json!({
                    "scheduleName": event.schedule_name,
                    "scheduledTime": event.scheduled_time,
                    "processedAt": chrono::Utc::now().to_rfc3339(),
                });
                let sanitized_schedule = event.schedule_name.replace('/', "_");
                let kv_key = format!("cron_event:{}", sanitized_schedule);
                let value = serde_json::to_vec(&record).into_alien_error().context(
                    BindingsErrorData::SerializationFailed {
                        message: "Failed to serialize cron event".to_string(),
                    },
                )?;
                kv.put(&kv_key, value, None).await?;
                info!(kv_key = %kv_key, "Stored cron event in KV");

                Ok(())
            }
        });
    }

    // Queue message handler - stores received messages in KV for test verification
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
                // Sanitize message ID: replace / with _ to comply with KV key validation rules
                let kv = ctx.get_bindings().load_kv("test-alien-kv").await?;
                let record = serde_json::json!({
                    "messageId": message.id,
                    "source": message.source,
                    "payload": String::from_utf8_lossy(&message.payload).to_string(),
                    "receiptHandle": message.receipt_handle,
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
                info!(kv_key = %kv_key, "Stored queue message in KV");

                Ok(())
            }
        });
    }

    // Echo command handler - simple command that echoes back params
    ctx.on_command("echo", |params: serde_json::Value| async move {
        info!(params = ?params, "Received echo command");
        Ok(params)
    });

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
            "message": "ARC small payload test completed successfully"
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
            "message": "ARC large payload test completed successfully",
            "largeResponseData": large_data,
            "bulkData": bulk_array,
        }))
    });

    info!("Registered event handlers");
}

/// Build the Axum router with all HTTP endpoints.
fn build_router(app_state: AppState) -> Router {
    Router::new()
        // Health and utility endpoints
        .route("/health", get(handlers::health::health_check))
        .route("/hello", get(handlers::health::hello))
        // Environment endpoints
        .route(
            "/env-var/{var_name}",
            get(handlers::environment::get_env_var),
        )
        // Testing utility endpoints
        .route("/inspect", post(handlers::inspect::inspect_request))
        .route("/sse", get(handlers::sse::sse_endpoint))
        // Test operation endpoints
        .route(
            "/storage-test/{binding_name}",
            post(handlers::storage::test_storage),
        )
        .route(
            "/build-test/{binding_name}",
            post(handlers::build::test_build),
        )
        .route(
            "/artifact-registry-test/{binding_name}",
            post(handlers::artifact_registry::test_artifact_registry),
        )
        .route("/kv-test/{binding_name}", post(handlers::kv::test_kv))
        .route(
            "/queue-test/{binding_name}",
            post(handlers::queue::test_queue),
        )
        .route(
            "/vault-test/{binding_name}",
            post(handlers::vault::test_vault),
        )
        .route(
            "/external-secret",
            get(handlers::vault::get_external_secret),
        )
        // Event verification endpoints (for tests to check received events)
        // Note: {*...} captures paths with slashes like "test/data-xxx.zip" or "arn:aws:events:.../rule/..."
        .route(
            "/events/storage/{*key}",
            get(handlers::events::get_storage_event),
        )
        .route(
            "/events/cron/{*schedule}",
            get(handlers::events::get_cron_event),
        )
        .route(
            "/events/queue/{*message_id}",
            get(handlers::events::get_queue_message),
        )
        .route("/events/list", get(handlers::events::list_events))
        // WaitUntil test endpoints
        .route(
            "/wait-until-test",
            post(handlers::wait_until::test_wait_until),
        )
        .route(
            "/wait-until-verify/{test_id}/{storage_binding_name}",
            get(handlers::wait_until::verify_wait_until),
        )
        // Add shared state
        .with_state(app_state)
}
