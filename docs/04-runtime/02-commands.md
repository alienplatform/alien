# Remote Commands

Remote commands let the control plane send commands to deployments running in environments you don't control.

## The Problem

Deployments run in environments where inbound networking is often blocked. You can't just call an HTTP endpoint. But you still need to:

- Trigger actions (sync data, generate reports)
- Get results back
- Handle failures and retries

Remote commands solve this with two delivery modes:

1. **Push** - Use platform-native mechanisms (Lambda invoke, Pub/Sub, Service Bus) that don't require inbound networking
2. **Poll** - Deployment pulls commands via outbound HTTPS (great for containers, can't be used in serverless functions)

Both modes use the same envelope format. The control plane doesn't care which mode the deployment uses.

## Commands

A command is a message from control plane to deployment:

- **name** - What to do (`generate-report`, `sync-data`)
- **params** - Input (JSON, can be large)
- **response** - Output (JSON, can be large) or error

From the Alien application's perspective:

```typescript
ctx.onCommand("generate-report", async (params) => {
  const report = await generateReport(params.startDate, params.endDate)
  return { report }
})
```

From the caller's perspective:

```typescript
const response = await commands.sendCommand(deploymentId, "generate-report", {
  startDate: "2024-01-01",
  endDate: "2024-01-31"
})
```

## Enabling Commands

In the stack definition:

```typescript
new alien.Function("my-function")
  .commandsEnabled(true)
  .build()
```

This triggers platform-specific infrastructure:

| Platform | What's Created |
|----------|----------------|
| AWS Lambda | Nothing extra - `InvokeFunction` is built-in |
| GCP Cloud Run | Pub/Sub topic + subscription (named `{function-name}-rq`) |
| Azure Container Apps | Service Bus queue + KEDA scaling (named `{function-name}-rq`) |
| Kubernetes | Nothing extra - uses polling |

**Container** also supports commands when built from source (includes `alien-runtime`). Container always uses polling - there's no push mechanism for long-running containers.

### Ingress + Commands

Commands work independently of the `ingress` setting:

- `ingress("private")` + `commandsEnabled(true)` → Function only reachable via commands (no public URL)
- `ingress("public")` + `commandsEnabled(true)` → Both public URL and commands work

## Delivery Modes

### Push

Platform-native mechanisms that don't require inbound networking:

| Platform | Mechanism |
|----------|-----------|
| AWS Lambda | `lambda:InvokeFunction` API |
| GCP Cloud Run | Pub/Sub push to private service |
| Azure Container Apps | Service Bus queue via Dapr |

The command server dispatches envelopes using these cloud APIs. The deployment's transport (see `04-runtime/00-runtime.md`) normalizes the platform-specific format and delivers the command to the application.

**Tradeoff:** Push requires cross-account access. The managing account needs permissions to invoke Lambda functions, publish to Pub/Sub, or send to Service Bus in the remote environment.

### Poll

For environments where push isn't available:

- **Kubernetes** - No push mechanism for pods
- **Local deployments** - Running on laptops, robots, edge devices
- **Zero cross-account access** - When the remote environment grants no permissions to the managing account

How it works:

1. Deployment polls the command server for commands (via `alien-runtime`)
2. Server returns pending commands as "leases"
3. Deployment processes command, submits response
4. Lease expires if not completed → command returns to pending

Enable with environment variables:

```
ALIEN_COMMANDS_POLLING_ENABLED=true
ALIEN_COMMANDS_POLLING_URL=https://server.example.com/v1/commands/leases
ALIEN_COMMANDS_POLLING_INTERVAL=5
```

## The Envelope

Commands are wrapped in an envelope for transport:

```json
{
  "protocol": "commands.v1",
  "commandId": "cmd_abc123",
  "attempt": 1,
  "deadline": "2025-12-31T23:59:59Z",
  "command": "generate-report",
  "params": {
    "mode": "inline",
    "inlineBase64": "eyJzdGFydERhdGUiOiIyMDI0LTAxLTAxIn0="
  },
  "responseHandling": {
    "maxInlineBytes": 150000,
    "submitResponseUrl": "https://server.example.com/v1/commands/cmd_abc123/response",
    "storageUploadUrl": "https://storage/responses/...",
    "storageUploadExpiresAt": "2025-09-01T13:00:00Z"
  }
}
```

Key fields:

- **protocol** - Version identifier (`commands.v1`)
- **commandId** - Unique ID for deduplication
- **attempt** - Incremented on retries
- **deadline** - Command expires if not completed by this time
- **params** - Input data (inline or storage URL)
- **responseHandling** - How to send the response back

## Payload Size Handling

The inline limit is **150KB** (raw bytes before base64). Why?

- Most conservative platform limit is Azure Service Bus Standard at 256KB
- Base64 encoding inflates by ~4/3: 150KB → ~200KB
- Leaves ~56KB headroom for envelope metadata
- Other platforms have higher limits (Lambda: 1MB, Pub/Sub: 10MB)

### Large Params (2-step upload)

When params exceed 150KB:

1. Client calls `POST /v1/commands` with `params: { mode: "storage", size: 500000 }`
2. Server returns `commandId` and `storageUpload: { putUrl: "https://...", expiresAt: "..." }`
3. Command enters **PENDING_UPLOAD** state
4. Client uploads data to `putUrl`
5. Client calls `POST /v1/commands/{id}/upload-complete` with `{ size: 500000 }`
6. Server moves command to **PENDING**, then dispatches → **DISPATCHED**

### Large Response (2-step upload)

When the deployment's response exceeds `maxInlineBytes` from envelope:

1. Deployment uploads response bytes to `storageUploadUrl` from envelope
2. Deployment calls `PUT /v1/commands/{id}/response` with:
   ```json
   {
     "status": "success",
     "response": { "mode": "storage", "size": 500000 }
   }
   ```
3. Control plane downloads from `storageGetUrl` when checking status

## Command States

```
PENDING_UPLOAD → PENDING → DISPATCHED → SUCCEEDED
                                     → FAILED
                                     → EXPIRED
```

- **PENDING_UPLOAD** - Large params: waiting for client to upload to storage and call `/upload-complete`
- **PENDING** - Ready for dispatch (or re-dispatch after lease expiry)
- **DISPATCHED** - Sent to deployment infrastructure (push) or leased (poll)
- **SUCCEEDED** - Deployment returned success response
- **FAILED** - Deployment returned error response
- **EXPIRED** - Deadline passed without response

For inline params, commands skip PENDING_UPLOAD and start at PENDING.

## Deduplication

Commands provide **at-least-once delivery**. Deployments may receive the same command multiple times.

`alien-runtime` deduplicates by `commandId` (caches recent IDs for 5 minutes). First valid response wins - later duplicates are ignored.

## Non-Blocking Server

The command server never long-polls. All endpoints return immediately:

- `POST /v1/commands` - Create command, returns `commandId`
- `GET /v1/commands/{id}` - Check status (client polls this)
- `PUT /v1/commands/{id}/response` - Deployment submits response
- `POST /v1/commands/{id}/upload-complete` - Client signals large params uploaded
- `POST /v1/commands/leases` - Deployment acquires commands (for polling mode)

This keeps the server cheap and elastic (scale-to-zero friendly).

## Client Behavior

The control plane client (generated from OpenAPI) follows these patterns:

### Polling for Status

```
Initial: 500ms
Backoff: multiply by 1.5 each iteration
Max: 5 seconds
```

The server never waits. Client is responsible for polling until terminal state.

### Idempotency

Use `idempotencyKey` when creating commands:

```typescript
await commands.createCommand({
  deploymentId: "dp_xyz",
  command: "sync-data",
  params: { ... },
  idempotencyKey: "sync-2024-01-15"  // Prevents duplicate commands
})
```

If a command with the same `idempotencyKey` already exists, the server returns the existing command instead of creating a new one.

### Retries

The command server doesn't retry failed commands automatically. If a command fails or expires, the client must create a **new command** to retry. This is intentional - the client has context about whether retrying makes sense.

---

## Implementation Details

The following sections are for developers building the command server and runtime integration.

### Crate Architecture

The `alien-commands` crate has two features:

**`alien-commands[server]`** - For embedding in a command server host:
- `CommandServer` - Core server with business logic
- KV/Storage integration via `alien-bindings` traits
- Pluggable dispatchers (Lambda, Pub/Sub, Service Bus)
- Axum router helpers
- OpenAPI schemas

**`alien-commands[runtime]`** - For alien-runtime:
- Envelope parsing and detection
- Response submission to the command server

The base crate (no features) provides protocol types: `Envelope`, `BodySpec`, `CommandResponse`, etc.

### CommandRegistry - Source of Truth

The `CommandRegistry` trait is the source of truth for command metadata. The crate provides one built-in implementation:

**`InMemoryCommandRegistry`** - For local development (`alien dev`):
- Stores metadata in-memory
- No persistence across restarts
- Same interface as platform registry

The registry tracks:
- Command state (Pending, Dispatched, Succeeded, Failed, Expired)
- Timestamps (created_at, dispatched_at, completed_at)
- Attempt count (incremented on retry)
- Deadline (optional expiry time)
- Error details (full JSON for failed commands)
- Deployment model (Push or Pull - captured from deployment at creation)

### KV Storage - Operational Data Only

The command server uses `alien-bindings::Kv` for operational data. Command metadata lives in the registry.

**Key schema:**

```
cmd:{command_id}:params                                  → bytes (large params blob)
cmd:{command_id}:response                                → bytes (large response blob)
cmd:{command_id}:lease                                   → LeaseData (with TTL)
target:{deployment_ref}:pending:{timestamp}:{command_id} → empty marker
idempotency:{key}                                        → command_id
```

The KV layer stores:
- Payload blobs (params, responses)
- Active leases (with TTL for automatic expiry)
- Pending command index (for pull-mode deployments)
- Idempotency keys (prevent duplicate commands)

### Core Operations

**Command creation flow:**

The command server orchestrates this:

```rust
async fn create_command(&self, request: CreateCommandRequest) -> Result<CreateCommandResponse> {
    // 1. Registry creates command and returns routing info
    let metadata = self.registry.create_command(
        &request.deployment_id,
        &request.command,
        initial_state,
        request.deadline,
        request_size_bytes
    ).await?;

    // 2. Store params blob in KV
    self.store_params(&metadata.command_id, &request.params).await?;

    // 3. Route based on deployment model from registry
    match metadata.deployment_model {
        DeploymentModel::Push => {
            self.dispatch_command_push(&metadata.command_id, &request.deployment_id).await?;
            (CommandState::Dispatched, "poll")
        }
        DeploymentModel::Pull => {
            self.create_pending_index(&request.deployment_id, &metadata.command_id).await?;
            (CommandState::Pending, "poll")
        }
    }
}
```

**Status check:**

```rust
async fn get_command_status(&self, command_id: &str) -> Result<CommandStatus> {
    // Registry is source of truth
    let status = self.registry.get_command_status(command_id).await?;

    // Check deadline expiry
    if let Some(deadline) = status.deadline {
        if Utc::now() > deadline && !status.state.is_terminal() {
            self.registry.update_command_state(
                command_id,
                CommandState::Expired,
                None, Some(Utc::now()), None, None
            ).await?;
            status.state = CommandState::Expired;
        }
    }

    Ok(status)
}
```

**Lease acquisition (pull mode):**

```rust
async fn acquire_lease(&self, deployment_id: &str) -> Result<Option<Envelope>> {
    // 1. Scan pending index
    let prefix = format!("target:{}:pending:", deployment_id);
    let items = self.kv.scan_prefix(&prefix, Some(10), None).await?.items;

    // 2. Try to acquire lease atomically
    for (index_key, _) in items {
        let cmd_id = extract_id(&index_key);
        let lease_key = format!("cmd:{}:lease", cmd_id);

        let lease = LeaseData {
            lease_id: generate_id(),
            expires_at: Utc::now() + Duration::minutes(5),
        };

        let options = PutOptions { if_not_exists: true, ttl: Some(Duration::minutes(5)) };
        if self.kv.put(&lease_key, to_vec(&lease)?, Some(options)).await? {
            // Success - get metadata from registry
            let metadata = self.registry.get_command_metadata(&cmd_id).await?;
            let params = self.kv.get(&format!("cmd:{}:params", cmd_id)).await?;

            // Update state in registry
            self.registry.update_command_state(
                &cmd_id,
                CommandState::Dispatched,
                Some(Utc::now()), None, None, None
            ).await?;

            return Ok(Some(build_envelope(metadata, params)));
        }
    }
    Ok(None)
}
```

### TTL and Cleanup

- **Lease keys** - TTL = 5 minutes. Expiry makes command available for re-lease
- **Pending index** - Cleaned up on successful lease acquisition or command completion
- **Command metadata** - Retained in registry per configured policy (default 30 days)
- **Payload blobs** - Cleaned up by background task after command reaches terminal state

### Storage Integration

Uses `alien-bindings::Storage` for large payloads:

- **Params upload** - Client gets presigned PUT URL, uploads, calls `/upload-complete`
- **Response upload** - Deployment uses `storageUploadUrl` from envelope
- **Cleanup** - Background task deletes blobs for terminal commands after retention period

### Dispatchers

Dispatchers send envelopes to push-mode deployments:

```rust
#[async_trait]
pub trait CommandDispatcher {
    async fn dispatch(&self, envelope: &Envelope) -> Result<()>;
}
```

Platform implementations:
- **AWS**: Calls `lambda:InvokeFunction`
- **GCP**: Publishes to Pub/Sub topic
- **Azure**: Sends to Service Bus queue

Pull-mode deployments (Kubernetes, Local) don't use dispatchers. They acquire leases by polling.

### Error Codes

| Code | Description |
|------|-------------|
| `INVALID_ARGUMENT` | Invalid command parameters |
| `NOT_FOUND` | Command or deployment not found |
| `CONFLICT` | Concurrent modification |
| `BAD_STATE` | Invalid state transition |
| `EXPIRED` | Command past deadline |
| `DEPLOYMENT_ERROR` | Deployment returned error |
| `STORAGE_ERROR` | Storage operation failed |
| `INTERNAL` | Server internal error |

### Protocol Versioning

- Envelope carries `protocol: "commands.v1"`
- Breaking changes require new version (`commands.v2`) and new endpoints
- Runtime must handle multiple versions during rollout

## Platform Integration

How commands flow through each platform (covered in more detail in `04-runtime/00-runtime.md`):

**AWS Lambda:**
1. Command server calls `lambda:InvokeFunction` with envelope
2. Lambda runtime delivers to `alien-runtime`
3. `lambda` transport detects command envelope
4. Runtime delivers to app via gRPC
5. App processes, returns response
6. Runtime submits response to command server

**GCP Cloud Run:**
1. Command server publishes to Pub/Sub
2. Pub/Sub pushes to Cloud Run (CloudEvent)
3. `cloudrun` transport detects command envelope
4. Runtime delivers to app via gRPC
5. App processes, returns response
6. Runtime submits response, returns 200 (acks Pub/Sub)

**Azure Container Apps:**
1. Command server publishes to Service Bus
2. KEDA scales container, Dapr pulls message
3. `containerapp` transport detects command envelope
4. Runtime delivers to app via gRPC
5. App processes, returns response
6. Runtime submits response, returns 200 (acks Service Bus)

**Kubernetes (polling):**
1. `alien-runtime` polls `ALIEN_COMMANDS_POLLING_URL`
2. Command server returns lease with envelope
3. Runtime delivers to app via gRPC
4. App processes, returns response
5. Runtime submits response to command server

## Cleanup and Retention

- Command metadata retained for configurable period (e.g., 7-30 days)
- Large payloads in storage cleaned up by background tasks
- Expired leases automatically make commands available for re-lease
