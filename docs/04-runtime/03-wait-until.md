# wait_until

`wait_until()` enqueues asynchronous tasks during a request's lifecycle. Tasks run in the background without blocking the response, but complete before the Function shuts down.

Use cases: flushing logs, sending analytics, updating caches, cleanup operations.

## The Problem

Functions are ephemeral. Platforms freeze or terminate them immediately after the response returns. Background work gets killed mid-execution.

## The Solution

`wait_until` registers background tasks and coordinates with the runtime to ensure they complete before shutdown:

1. App registers tasks via `wait_until()`
2. Runtime tracks active tasks
3. On shutdown, runtime signals "drain now"
4. App completes tasks within timeout
5. Runtime confirms completion before exit

```
Application                Runtime                  Platform
    │                         │                        │
    │── task registered ─────→│                        │
    │                         │                        │
    │                         │←───── shutdown ────────│
    │                         │                        │
    │←──── drain signal ──────│                        │
    │                         │                        │
    │── drain complete ──────→│                        │
    │                         │                        │
    │                         │───── exit ────────────→│
```

## Usage

Applications access wait_until through `AlienContext`:

```rust
let ctx = AlienContext::from_env().await?;

// In request handlers
ctx.wait_until(|| async move {
    flush_metrics().await;
    send_analytics().await;
})?;
```

Tasks start immediately. They run in the application process—no serialization, no network overhead.

## Architecture

Two components coordinate via gRPC:

**WaitUntilContext** (in application)
- Spawns tasks with `tokio::spawn`
- Tracks active tasks in a HashMap
- Listens for drain signals
- Reports completion status

**WaitUntilGrpcServer** (in runtime)
- Tracks task counts per application
- Sends drain signals when shutdown triggers
- Waits for completion before allowing exit

### gRPC Protocol

```protobuf
service WaitUntilService {
    // App notifies runtime of new task
    rpc NotifyTaskRegistered(NotifyTaskRegisteredRequest) returns (NotifyTaskRegisteredResponse);
    
    // App blocks waiting for drain signal
    rpc WaitForDrainSignal(WaitForDrainSignalRequest) returns (WaitForDrainSignalResponse);
    
    // App confirms drain complete
    rpc NotifyDrainComplete(NotifyDrainCompleteRequest) returns (NotifyDrainCompleteResponse);
    
    // Runtime queries task count
    rpc GetTaskCount(GetTaskCountRequest) returns (GetTaskCountResponse);
}
```

The application calls `WaitForDrainSignal` at startup. This blocks until the runtime decides it's time to drain. The runtime stores a oneshot channel sender for each application—when shutdown triggers, it sends the drain signal through this channel.

## Platform Triggers

Each platform triggers drain differently:

| Platform | Trigger | Implementation |
|----------|---------|----------------|
| Lambda | After each invocation | Internal extension |
| Cloud Run | SIGTERM | Signal handler |
| Azure | SIGTERM | Signal handler |
| Local | Ctrl+C | Signal handler |

### Lambda

Lambda uses an internal extension registered with the Lambda Extensions API. The extension:

1. Receives INVOKE events after the handler returns
2. Calls `WaitUntilGrpcServer::trigger_drain_all("lambda_invoke_end", 10)`
3. Waits for applications to drain before the next freeze

```rust
// In lambda.rs
async fn invoke(&self, event: LambdaEvent) -> Result<()> {
    // Wait for handler to complete
    self.request_done_receiver.lock().await.recv().await?;
    
    // Trigger drain
    if let Some(server) = get_wait_until_server() {
        server.trigger_drain_all("lambda_invoke_end", 10).await?;
    }
    
    Ok(())
}
```

The extension coordinates with the handler via an mpsc channel. The handler signals completion, then the extension triggers drain.

### Cloud Run / Azure / Local

These platforms send SIGTERM (or Ctrl+C for local). The runtime's shutdown handler triggers drain:

```rust
// In runtime.rs
shutdown_result = shutdown_rx.recv() => {
    if let Some(wait_until_server) = wait_until_server_handle {
        wait_until_server.trigger_drain_all("shutdown", 10).await?;
        
        // Wait for tasks to complete (up to 15s)
        loop {
            if wait_until_server.get_total_task_count().await == 0 {
                break;
            }
            if elapsed >= max_wait_time { break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}
```

## Drain Flow

When drain triggers:

1. **Runtime** calls `trigger_drain_all(reason, timeout_secs)`
2. **Server** sends drain signal to all registered applications via oneshot channels
3. **Application** receives signal in `start_drain_listener()` background task
4. **Application** sets `draining = true` to reject new tasks
5. **Application** calls `drain_all()` which awaits all JoinHandles with timeout
6. **Application** notifies runtime via `NotifyDrainComplete`
7. **Server** decrements task counts

```rust
// In WaitUntilContext::drain_all()
async fn drain_all(&self, config: DrainConfig) -> Result<DrainResponse> {
    // Mark draining to reject new tasks
    *self.draining.lock().await = true;
    
    // Take all tasks
    let tasks_to_drain = std::mem::take(&mut *self.tasks.lock().await);
    
    // Wait with timeout
    timeout(config.timeout, async {
        for (task_id, handle) in tasks_to_drain {
            handle.await?;
        }
    }).await;
    
    // Reset flag
    *self.draining.lock().await = false;
    
    Ok(DrainResponse { tasks_drained, success, error_message })
}
```

## Initialization

The runtime starts the gRPC server before the application handler:

```rust
// In runtime.rs
let (wait_until_server, grpc_task, readiness_rx) = run_grpc_server(provider, &addr).await?;

// Store for Lambda transport access
WAIT_UNTIL_SERVER.set(wait_until_server.clone())?;

// Wait for server ready
readiness_rx.await?;

// Now start the application
config.handler.start().await?;
```

The application connects via `ALIEN_BINDINGS_GRPC_ADDRESS` environment variable (set by runtime):

```rust
// In WaitUntilContext::from_env_with_vars()
let grpc_address = env_vars
    .get("ALIEN_BINDINGS_GRPC_ADDRESS")
    .unwrap_or("http://127.0.0.1:51351");

let channel = Channel::from_shared(grpc_address)?
    .timeout(Duration::from_secs(300))  // Long timeout for drain signal RPC
    .connect().await?;
```

## Timeouts

| Timeout | Value | Purpose |
|---------|-------|---------|
| Drain timeout | 10s | Time apps have to complete tasks |
| Wait for drain | 15s | Runtime waits before forcing exit |
| gRPC timeout | 5min | Long-lived WaitForDrainSignal RPC |

## Scope

wait_until applies to **Function** resources only. Containers and Workers don't need it—they're always-on and handle their own lifecycle.

