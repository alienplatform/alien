# Rust SDK

The Rust SDK is **explicit** - everything goes through `AlienContext`. No global state.

### Basic Setup

```rust
use alien_bindings::AlienContext;

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = AlienContext::from_env().await?;
    
    // ... register handlers, start HTTP ...
    
    ctx.run().await
}
```

### Bindings

Load through the context:

```rust
let storage = ctx.bindings().load_storage("my-bucket").await?;
storage.put(&"key".into(), data).await?;

let kv = ctx.bindings().load_kv("my-cache").await?;
kv.get("key").await?;
```

### Events

Register handlers, then call `ctx.run()`:

```rust
ctx.on_storage_event("uploads", |event| async move {
    println!("File {} was {}", event.key, event.event_type);
    Ok(())
});

ctx.on_cron_event("daily-cleanup", || async move {
    cleanup_old_files().await;
    Ok(())
});

ctx.on_queue_message("tasks", |message| async move {
    process_task(message.payload).await;
    Ok(())
});
```

### Commands

```rust
ctx.on_command("health-check", |_params: Value| async move {
    Ok(json!({ "healthy": true }))
});

ctx.on_command("generate-report", |params: GenerateReportParams| async move {
    let url = generate_report(params.report_type).await?;
    Ok(json!({ "url": url }))
});
```

### HTTP Server

The SDK doesn't wrap HTTP frameworks. You start your server and tell the context the port:

```rust
use axum::{routing::get, Router};
use tokio::net::TcpListener;

let app = Router::new().route("/", get(|| async { "Hello!" }));

// Bind to random port
let listener = TcpListener::bind("127.0.0.1:0").await?;
let port = listener.local_addr()?.port();

// Tell runtime about the HTTP server
ctx.register_http_server(port).await?;

// Run HTTP server in background
tokio::spawn(async move {
    axum::serve(listener, app).await.unwrap();
});

// Enter event loop (blocks until shutdown)
ctx.run().await?;
```

This works with any HTTP framework (Axum, Poem, Actix, etc.) - just bind, register the port, spawn, and run.

### waitUntil

```rust
ctx.wait_until(async move {
    heavy_processing().await;
});
```

### Full Example

```rust
use alien_bindings::AlienContext;
use axum::{routing::get, Router};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<()> {
    let ctx = AlienContext::from_env().await?;
    
    // Events
    ctx.on_storage_event("uploads", |event| async move {
        let storage = ctx.bindings().load_storage("uploads").await?;
        let data = storage.get(&event.key).await?;
        process_file(data).await;
        Ok(())
    });
    
    // Commands
    ctx.on_command("health", |_| async move {
        Ok(json!({ "healthy": true }))
    });
    
    // HTTP
    let app = Router::new().route("/", get(|| async { "Hello!" }));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    ctx.register_http_server(listener.local_addr()?.port()).await?;
    tokio::spawn(axum::serve(listener, app));
    
    ctx.run().await
}
```

---
