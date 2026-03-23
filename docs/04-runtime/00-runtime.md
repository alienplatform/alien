# Alien Runtime

Every container or function that Alien deploys runs `alien-runtime` as its entry point. The runtime starts your application as a subprocess, then acts as the bridge between your app and the cloud platform.

```dockerfile
ENTRYPOINT ["alien-runtime", "--"]
CMD ["bun", "index.ts"]
```

This is the `ENTRYPOINT` in every Alien-built image. The `--` separates runtime flags from the app command. When Lambda (or Cloud Run, or a local machine) starts the container, it starts the runtime — which in turn starts your app.

The app is written using standard code. The runtime handles all platform differences.

## The Contract

The contract between runtime and app is **stable**. Breaking it breaks all Alien applications built by developers.

Why this matters: We control the platform side (e.g., how Lambda events are parsed, how Cloud Run receives Pub/Sub). We can change that anytime. But we don't control the apps developers build. If we change what `AlienContext.fromEnv()` expects, we break their code.

### What the App Expects

**1. gRPC server at `ALIEN_BINDINGS_GRPC_ADDRESS`**

```typescript
const ctx = await AlienContext.fromEnv()  // Connects to gRPC
```

**2. Bindings via gRPC**

```typescript
const storage = await ctx.bindings.loadStorage("bucket")
const kv = await ctx.bindings.loadKv("cache")
```

**3. Events and commands via gRPC**

```typescript
ctx.onStorageEvent("uploads", async (event) => { ... })
ctx.onCommand("run-report", async (params) => { ... })
```

The SDK calls `WaitForEvents()` - a blocking gRPC call. The runtime delivers events through it.

**4. waitUntil via gRPC**

```typescript
ctx.waitUntil(async () => await sendAnalytics())
```

The SDK calls `NotifyTaskRegistered()`, then `WaitForDrainSignal()` to know when to drain.

**5. HTTP port (optional)**

If the app has an HTTP server, it tells the runtime:

```typescript
app.listen(3000)
ctx.registerHttpServer(3000)
```

The runtime forwards HTTP to this port (for `lambda`, `cloudrun`, `containerapp` transports).

**6. Secrets as environment variables**

If `ALIEN_SECRETS` is set, runtime loads secrets from vault before starting the app. Available via `process.env`.

### Contract Summary

| What | How |
|------|-----|
| Bindings | gRPC calls to runtime |
| Events/Commands | `WaitForEvents()` blocking call |
| waitUntil | `NotifyTaskRegistered()` + `WaitForDrainSignal()` |
| HTTP port | `RegisterHttpServer(port)` |
| Secrets | Loaded before app starts, set as env vars |
| Shutdown | `WaitForDrainSignal()` returns |

## Architecture

The runtime exists in two forms:

**1. Standalone Process (`alien-runtime` binary)**

Used in cloud platforms (Lambda, Cloud Run, Container App):
- Each function invocation runs in its own process
- Configuration from environment variables
- Secrets loaded from vault before app starts

**2. Embedded Library (`alien_runtime::run()`)**

Used in Local Platform (Agent, `alien dev`):
- One process (Agent/CLI) with multiple runtime tasks
- Each runtime task spawns its own app subprocess
- Configuration passed programmatically via `RuntimeConfig`
- Bindings provided via `BindingsSource::Provider`

### How They Differ

|| Standalone | Embedded |
|---|-----------|----------|
| **Process model** | One process per function | One process, multiple runtime tasks |
| **Config source** | Environment variables + CLI args | `RuntimeConfig` struct |
| **Bindings** | `BindingsSource::FromEnvironment` | `BindingsSource::Provider(provider)` |
| **Secrets** | Loaded from vault (ALIEN_SECRETS) | Passed directly in env_vars |
| **App subprocess** | Inherits runtime process env | Isolated via `Command::env()` |

The runtime sits between the platform and the app. It translates platform-specific inputs into a consistent interface.

```
                        alien-runtime
                    ┌───────────────────┐
                    │                   │
  Platform ────────►│    Transport      │
  (Lambda API,      │    (normalizes)   │
   CloudEvents,     │         │         │
   HTTP, etc.)      │         ▼         │
                    │   ┌───────────┐   │
                    │   │  Runtime  │   │      ┌─────────┐
                    │   │   Core    │───┼─────►│   App   │
                    │   └───────────┘   │ gRPC │(process)│
                    │         │         │      └─────────┘
                    │         ▼         │
                    │   ┌───────────┐   │
                    │   │ Bindings  │◄──┼────── App calls
                    │   │  Server   │   │       bindings
                    │   └───────────┘   │
                    │                   │
                    └───────────────────┘
```

### Transport

Each platform delivers work differently. The transport's job is to normalize these differences.

| Platform | How it receives work |
|----------|---------------------|
| Lambda | Polls Lambda Runtime API |
| Cloud Run | HTTP server receives CloudEvents |
| Container App | HTTP server receives Dapr messages |
| Local | HTTP server forwards to app (no CloudEvents) |
| Passthrough | No transport - app handles HTTP directly |

The transport produces normalized types (`StorageEvent`, `QueueMessage`, `CronEvent`, `Command`, or HTTP requests). The runtime core handles delivery to the app.

### Runtime Core

Coordinates everything:

- Starts gRPC server for bindings
- Loads secrets from vault
- Starts app subprocess
- Starts transport
- Routes normalized types to the app (HTTP forwarding or gRPC delivery)

## Transports

### Lambda

`ALIEN_TRANSPORT=lambda`

| From Platform | Normalized To |
|---------------|---------------|
| API Gateway | HTTP request |
| S3 event | `StorageEvent` |
| SQS message | `QueueMessage` |
| CloudWatch scheduled | `CronEvent` |
| InvokeFunction (command) | `Command` |

Config:
- `ALIEN_LAMBDA_MODE`: `buffered` (default) or `streaming`

### Cloud Run

`ALIEN_TRANSPORT=cloudrun`

| From Platform | Normalized To |
|---------------|---------------|
| HTTP request | HTTP request |
| GCS CloudEvent | `StorageEvent` |
| Pub/Sub CloudEvent | `QueueMessage` |
| Cloud Scheduler | `CronEvent` |
| Pub/Sub (command) | `Command` |

Config:
- `ALIEN_CLOUDRUN_PORT`: Port to listen on

### Container App

`ALIEN_TRANSPORT=containerapp`

| From Platform | Normalized To |
|---------------|---------------|
| HTTP request | HTTP request |
| Blob CloudEvent | `StorageEvent` |
| Service Bus (Dapr) | `QueueMessage` |
| Timer trigger | `CronEvent` |
| Service Bus (command) | `Command` |

Config:
- `ALIEN_CONTAINERAPP_PORT`: Port to listen on

### Local

`ALIEN_TRANSPORT=local`

Simple HTTP proxy. Forwards all HTTP requests to the application. No CloudEvents parsing, no platform-specific middleware.

| From Platform | Normalized To |
|---------------|---------------|
| HTTP request | HTTP request |

Config:
- `PORT`: Port to listen on (default: 8080)

Used for: Local Platform deployments (VMs, edge devices, bare metal), local development.

### Passthrough

`ALIEN_TRANSPORT=passthrough`

No transport. App handles external HTTP directly. Runtime provides bindings only.

Used for: Kubernetes, containers, workers.

## Command Polling

Independent of transport. For environments that can't receive pushes.

```
ALIEN_COMMANDS_POLLING_ENABLED=true
ALIEN_COMMANDS_POLLING_URL=https://server.example.com/v1/commands/leases
ALIEN_COMMANDS_POLLING_INTERVAL=5
```

The runtime polls for commands and delivers them via gRPC, same as push-based commands.

## Configuration

Configuration via environment variables or CLI arguments (using `clap`). In practice, environment variables are more convenient since the Dockerfile stays the same across platforms.

### Dockerfile

```dockerfile
ENTRYPOINT ["alien-runtime", "--"]
CMD ["bun", "index.ts"]
```

`ENTRYPOINT` is always the runtime. `CMD` comes from the stack definition.

### Core Variables

| Variable | Purpose |
|----------|---------|
| `ALIEN_TRANSPORT` | `lambda`, `cloudrun`, `containerapp`, `local`, or `passthrough` |
| `ALIEN_BINDINGS_GRPC_ADDRESS` | gRPC address for bindings (default: `127.0.0.1:51351`) |
| `ALIEN_SECRETS` | JSON with secret keys to load |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP telemetry endpoint |

### Transport Variables

**Lambda:**
| Variable | Purpose |
|----------|---------|
| `ALIEN_LAMBDA_MODE` | `buffered` (default) or `streaming` |

**Cloud Run / Container App:**
| Variable | Purpose |
|----------|---------|
| `PORT` | Port to listen on (default: 8080) |

### Command Polling Variables

| Variable | Purpose |
|----------|---------|
| `ALIEN_COMMANDS_POLLING_ENABLED` | `true` to enable |
| `ALIEN_COMMANDS_POLLING_URL` | Leases endpoint |
| `ALIEN_COMMANDS_POLLING_INTERVAL` | Seconds between polls |

## Lifecycle

### Startup

The gRPC server and secrets must be ready before the app starts. The app expects to connect to bindings immediately via `AlienContext.fromEnv()`.

Once the app is running, the runtime waits for `RegisterHttpServer()` if the transport needs HTTP forwarding. Only then does the transport start accepting work.

### Shutdown

Graceful shutdown lets the app finish in-flight work:

1. Runtime stops accepting new requests
2. `WaitForDrainSignal()` returns - app knows to stop taking new work
3. Runtime waits for all `waitUntil` tasks to complete
4. App terminates

## gRPC Communication

The runtime exposes gRPC services for bindings and control. Each resource type (Storage, KV, Queue, Vault) has its own service with operations specific to that resource.

The control service handles:
- HTTP server registration
- Event handler registration
- Event/command delivery (streaming)
- Drain signaling for graceful shutdown

See `alien-bindings/proto/*.proto` for the actual service definitions.

