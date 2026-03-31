# Commands

Remote commands let you invoke code on running deployments. Deployments run where inbound networking is blocked — a customer's VPC, an airgapped cluster — so commands use an outbound polling model. Zero inbound connections required.

## Defining Commands

Deployments register command handlers using the SDK:

```typescript
import { command } from "@alienplatform/sdk"

command("generate-report", async ({ startDate }) => {
  const data = await queryDatabase(startDate)
  return { report: formatReport(data) }
})
```

The runtime handles lease polling, command execution, and response submission automatically.

## Sending Commands

From the CLI:

```bash
alien command invoke \
  --server http://localhost:8080 \
  --deployment production \
  --command generate-report \
  --params '{"startDate": "2025-01-01"}'
```

Or via the REST API:

```
POST /v1/commands
```

```json
{
  "deploymentId": "dp_xxx",
  "name": "generate-report",
  "params": { "startDate": "2025-01-01" },
  "timeoutSeconds": 30
}
```

## How It Works

```
Caller                         alien-manager                  Deployment (remote)
     │                               │                              │
     │── POST /v1/commands ────────▶│  Create command               │
     │                               │                              │
     │                               │◀── POST /v1/commands/leases ─│
     │                               │    "Any work for me?"        │
     │                               │──── [command envelope] ─────▶│
     │                               │                              │
     │                               │                              │── execute handler
     │                               │                              │
     │                               │◀── PUT /v1/commands/         │
     │                               │    {id}/response              │
     │                               │    [result]                   │
     │                               │                              │
     │── GET /v1/commands/{id} ────▶│  Check status                │
     │◀── { state: "succeeded" } ───│                              │
```

### Lifecycle

1. **Created** — command stored in database, state = `pending`
2. **Dispatched** — deployment picks up via lease polling, state = `dispatched`, deadline timer starts
3. **Succeeded** / **Failed** — deployment submits response, state = `succeeded` or `failed`
4. **Expired** — deadline passed without response, state = `expired`

Deployments poll `POST /v1/commands/leases` periodically to check for pending commands. Each lease has a deadline — if the deployment doesn't respond in time, the command transitions to `expired`.

If a deployment releases a lease without completing the command (e.g., on shutdown), the command returns to `pending` for retry.

### Push Dispatch

For lower latency, alien-manager can push commands directly to a deployment's compute endpoint instead of waiting for the next poll cycle. The `DefaultCommandDispatcher` handles this:

1. Looks up the deployment's stack state to find the push endpoint (Lambda function ARN, Pub/Sub topic, Service Bus queue)
2. Resolves credentials for the target environment via `CredentialResolver`
3. Dispatches the command via the platform-specific mechanism

| Platform | Mechanism |
|----------|-----------|
| AWS | `lambda:InvokeFunction` (async) on the deployment's function ARN |
| GCP | Publish message to the deployment's Pub/Sub topic |
| Azure | Send message to the deployment's Service Bus queue |

Push dispatch requires two things: a push endpoint in the deployment's stack state, and credentials to reach it. Deployments without a push endpoint (Kubernetes, local, or deployments that haven't finished provisioning) fall back to lease polling.

`NullCommandDispatcher` is available for pull-only setups where no push dispatch is needed. Deployments still poll as a fallback even when push dispatch is configured.

### Dispatch Rules

The deployment model (push vs pull) does NOT affect command dispatch. All deployments poll for commands via the lease API. Push dispatch is an optimization for lower latency — it notifies the deployment immediately instead of waiting for the next poll cycle.

| Target | Dispatch | Mechanism |
|--------|----------|-----------|
| AWS Lambda | Push | `lambda:InvokeFunction` (async) |
| GCP Cloud Run | Push | Pub/Sub topic |
| Azure Container App | Push | Service Bus queue |
| K8s / Local containers | Poll | `ALIEN_COMMANDS_POLLING_URL` lease API |

Push-capable targets (Lambda, Cloud Run, Container App) also poll as a fallback. If push dispatch fails or is unavailable, the next poll cycle picks up the command.

## Large Payloads

Command params and responses under 150KB are stored inline. Larger payloads go to blob storage (Command Storage) and the command envelope contains a reference URL instead.

## Implementation

```sql
CREATE TABLE commands (
  id                  TEXT PRIMARY KEY,
  deployment_id       TEXT NOT NULL,
  name                TEXT NOT NULL,      -- command name (e.g., "generate-report")
  state               TEXT NOT NULL,      -- "pending" | "dispatched" | "succeeded" | "failed" | "expired"
  deployment_model    TEXT NOT NULL,      -- "push" | "pull"
  attempt             INTEGER DEFAULT 1,
  deadline            TEXT,
  created_at          TEXT DEFAULT CURRENT_TIMESTAMP,
  dispatched_at       TEXT,
  completed_at        TEXT,
  request_size_bytes  INTEGER,
  response_size_bytes INTEGER,
  error               TEXT                -- JSON: error details
)
```

The command server (from the `alien-commands` crate) manages the full lifecycle. It uses two pluggable components:

**CommandRegistry** — persists command metadata. Default: `SqliteCommandRegistry` — stores in the same SQLite database as other entities.

**CommandDispatcher** — pushes commands to deployments for lower latency. Default: `DefaultCommandDispatcher` — pushes via Lambda invoke, Pub/Sub, or Service Bus (see Push Dispatch above). `NullCommandDispatcher` available for pull-only setups.

Two storage backends hold command data:

- **Command KV** — command state, lease indices, pending queues. Small, fast key-value operations. Default: local filesystem.
- **Command Storage** — large command params and responses (>150KB). Default: local filesystem.
