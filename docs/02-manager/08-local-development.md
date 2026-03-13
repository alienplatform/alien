# Local Development

`alien dev` starts alien-manager in dev mode, builds your stack, and deploys everything locally with Docker. No cloud credentials, no container registry, no external services.

## What Happens

```bash
cd my-app
alien dev
```

1. Start alien-manager in dev mode (SQLite at `.alien/dev.db`, port 9090)
2. Build the stack for the local platform (`alien build --platform local`)
3. Create a release from the built OCI tarballs
4. Create a deployment in the default deployment group
5. Deployment loop picks it up, loads images into Docker, runs containers
6. CLI TUI shows deployment status and streams logs

Everything runs on your machine. The deployment loop uses `ClientConfig::Local` — no cloud APIs, just Docker.

## Dev Mode Differences

| Aspect | Standalone | Dev Mode |
|---|---|---|
| Database | `{DATA_DIR}/alien.db` | `.alien/dev.db` |
| Deployment loop interval | 10s | 1s |
| Credential resolver | `EnvironmentCredentialResolver` | `LocalCredentialResolver` |
| Telemetry | Forward to OTLP endpoint | In-memory LogBuffer |
| Auth | Token validation (SHA-256) | Permissive (any token accepted) |
| Container images | Pulled from registry | Loaded from local OCI tarballs |

## Log Streaming

`alien dev` runs alien-manager in the same process. Logs flow through a shared in-memory buffer — no HTTP APIs needed for reading.

### Architecture

```
Docker containers ──(stdout/stderr)──▶ ┐
                                       ├── LogBuffer (Arc, in-process) ──▶ TUI
alien-runtime containers ──(OTLP)────▶ ┘
```

The `LogBuffer` is a shared struct passed to both alien-manager and the TUI at startup:

```rust
struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,   // ring buffer, max 10K entries
    tx: broadcast::Sender<LogEntry>,      // notify TUI of new entries
}
```

### Two ingestion paths

1. **Docker log capture** — after the deployment loop starts a container, it spawns a task that tails Docker logs via the Docker API. `stdout` → INFO, `stderr` → ERROR. Works for all containers, even those without `alien-runtime`.

2. **OTLP ingestion** — containers with `alien-runtime` send structured OTLP logs to `POST /v1/logs` on the dev server. The dev-mode `TelemetryBackend` parses the protobuf and pushes entries into the same LogBuffer.

```rust
struct DevTelemetryBackend {
    log_buffer: Arc<LogBuffer>,
}

impl TelemetryBackend for DevTelemetryBackend {
    async fn ingest_logs(&self, data: Bytes, _scope: &str) -> Result<()> {
        let entries = parse_otlp_logs(data);
        self.log_buffer.push(entries);
        Ok(())
    }
    // traces/metrics: discard
}
```

### How the TUI reads logs

The TUI subscribes to the broadcast channel at startup:

```rust
let mut rx = log_buffer.tx.subscribe();
loop {
    let entry = rx.recv().await?;
    tui.append_log(entry);
}
```

No polling, no SSE, no HTTP. Just an in-process broadcast channel.

## Deployment Status

The TUI needs to show deployment status (running, provisioning, error) and resource URLs. Since `alien dev` embeds alien-manager in the same process, this uses a `watch` channel — no files, no polling.

### Architecture

```
Deployment loop ──(after reconcile)──▶ watch::Sender<DevStatus> ──▶ TUI
```

The CLI creates the channel and passes the sender to alien-manager:

```rust
let (status_tx, status_rx) = tokio::sync::watch::channel(DevStatus::default());

let server = AlienManagerBuilder::new(config)
    .dev_status(status_tx)
    .build()
    .await?;

// TUI reads from status_rx
```

### Hook point

After the deployment loop calls `reconcile()`, it checks if a `dev_status` sender is configured. If so, it reads deployment states from the store and publishes:

```rust
// In deployment loop, after reconcile
if let Some(tx) = &self.dev_status_tx {
    let deployments = self.store.list_deployments().await?;
    let status = DevStatus::from_deployments(&deployments);
    let _ = tx.send(status);
}
```

The `DevStatus` struct:

```rust
struct DevStatus {
    state: DevState,  // Initializing, Ready, Error
    deployments: Vec<DeploymentStatus>,
}

struct DeploymentStatus {
    name: String,
    status: String,
    resources: HashMap<String, ResourceStatus>,
}

struct ResourceStatus {
    url: Option<String>,   // public URL if applicable
    status: String,        // running, pending, error
}
```

Overall state: `Ready` if any deployment is `running`, `Error` if any has an error, `Initializing` otherwise.

### What AlienManagerBuilder needs

One new optional field:

```rust
impl AlienManagerBuilder {
    pub fn dev_status(mut self, tx: watch::Sender<DevStatus>) -> Self {
        self.dev_status_tx = Some(tx);
        self
    }
}
```

The deployment loop stores this and calls it after each reconcile. Standalone mode and managed mode don't set it — the field stays `None` and no work happens.

### Optional status file for external consumers

When `alien dev` is invoked by external tooling (e.g., the testing framework), the TUI is disabled and there's no in-process consumer for the `watch` channel. For these cases, `--status-file` writes the `DevStatus` as JSON after each update:

```bash
alien dev --no-tui --status-file .alien/dev-status.json
```

The CLI subscribes to the `watch::Receiver` and writes the JSON file atomically (write to `.tmp`, rename) on each change. The testing framework polls this file to discover deployment status and resource URLs.

```json
{
  "state": "Ready",
  "deployments": [
    {
      "name": "default",
      "status": "running",
      "resources": {
        "api": { "url": "http://localhost:3000", "status": "running" },
        "worker": { "status": "running" }
      }
    }
  ]
}
```

This is a CLI concern, not a server concern — the server only knows about the `watch` channel. The CLI decides whether to render to TUI, write to file, or both.

## Environment Variables

The deployment loop injects the same environment variables as standalone mode (see [Deployments — Environment Variables](01-deployments.md#environment-variables-injected-into-containers)), with localhost URLs:

```bash
ALIEN_DEPLOYMENT_ID=dp_xxx
ALIEN_TOKEN=ax_deploy_...
ALIEN_COMMANDS_POLLING_ENABLED=true
ALIEN_COMMANDS_POLLING_URL=http://localhost:9090/v1/commands/leases
OTEL_EXPORTER_OTLP_LOGS_ENDPOINT=http://localhost:9090/v1/logs
OTEL_EXPORTER_OTLP_HEADERS=authorization=Bearer ax_deploy_...
```

Plus user-supplied variables from `--env KEY=VALUE` and `--secret KEY=VALUE`.

## State Directory

All dev mode state lives in `.alien/`:

```
.alien/
  dev.db              # SQLite database
  build/              # Built OCI tarballs
  command_kv/         # Command server KV state
  command_storage/    # Command server storage
```
