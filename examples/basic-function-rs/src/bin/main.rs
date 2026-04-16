use alien_sdk::AlienContext;
use axum::{routing::get, Json, Router};
use serde_json::{json, Value};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let ctx = AlienContext::from_env()
        .await
        .expect("Failed to create Alien context");

    info!(app_id = %ctx.application_id(), "Initialized Alien context");

    // Register command handler
    ctx.on_command("echo", |params: Value| async move { Ok(params) });

    // Build HTTP router
    let app = Router::new().route("/health", get(|| async { Json(json!({"status": "ok"})) }));

    // Bind to dynamic port and register with runtime
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await?;
    let port = listener.local_addr()?.port();

    if let Err(e) = ctx.register_http_server(port).await {
        tracing::warn!(error = %e, "Failed to register HTTP server with runtime");
    }

    info!(port = port, "Server ready");

    // Start the event loop in background
    let ctx = Arc::new(ctx);
    let ctx_for_events = ctx.clone();
    tokio::spawn(async move {
        if let Err(e) = ctx_for_events.run().await {
            tracing::error!(error = %e, "Event loop error");
        }
    });

    axum::serve(listener, app).await?;
    Ok(())
}
