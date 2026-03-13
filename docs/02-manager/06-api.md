# REST API

Every endpoint in alien-manager's REST API. All requests require a `Bearer` token in the `Authorization` header (except health checks). See [Authentication](05-auth.md) for token types and scope enforcement.

Responses use JSON. Errors return `{ "error": "message" }` with appropriate HTTP status codes.

## Deployments

### Create Deployment

```
POST /v1/deployments
```

Creates a new deployment in a deployment group. The deployment loop picks it up and begins provisioning.

**Request:**
```json
{
  "name": "production",
  "platform": "aws",
  "deploymentGroupId": "dg_xxx",
  "stackSettings": {
    "deploymentModel": "push",
    "heartbeats": "on",
    "updates": "auto",
    "network": { }
  }
}
```

**Response (201):**
```json
{
  "deployment": {
    "id": "dp_xxx",
    "name": "production",
    "platform": "aws",
    "status": "pending",
    "deploymentGroupId": "dg_xxx",
    "createdAt": "2025-01-01T00:00:00Z"
  },
  "token": "ax_deploy_..."
}
```

The `token` field is present when the request uses a deployment group token. This deployment-scoped token is used by the deployment for OTLP log ingestion and command polling.

**Auth:** Admin or deployment group token.

### List Deployments

```
GET /v1/deployments
```

**Query params:**
- `deploymentGroupId` — filter by deployment group
- `include[]` — `deploymentGroup`, `release` (include related records)

**Auth:** Admin or deployment group token (own group only).

### Get Deployment

```
GET /v1/deployments/{id}
```

Returns the full deployment record including `stackState`, `stackSettings`, `environmentInfo`, `error`.

### Get Deployment Info

```
GET /v1/deployments/{id}/info
```

Returns runtime discovery information: command endpoint URL and resource public URLs.

```json
{
  "commands": {
    "url": "http://localhost:9090/v1/commands",
    "deploymentId": "dp_xxx"
  },
  "resources": {
    "router": {
      "type": "container",
      "publicUrl": "http://localhost:3000"
    }
  },
  "status": "running",
  "platform": "local"
}
```

Used by CLIs and SDKs to discover where to send commands and how to reach deployed resources.

### Delete Deployment

```
DELETE /v1/deployments/{id}
```

Sets status to `delete-pending`. The deployment loop handles teardown.

**Auth:** Admin token only.

### Retry Deployment

```
POST /v1/deployments/{id}/retry
```

Sets `retry_requested = true` on a failed deployment, causing the deployment loop to retry.

### Redeploy

```
POST /v1/deployments/{id}/redeploy
```

Triggers redeployment of a running deployment with the same release. Sets status to `update-pending`, causing the deployment loop to re-run `step()` and re-provision all resources.

Use this to force a re-provision when a deployment is running but needs to be refreshed (e.g., after infrastructure changes outside of Alien).

**Auth:** Admin or deployment group token (own group).

## Releases

### Create Release

```
POST /v1/releases
```

Creates a release record after building and pushing OCI images.

**Request:**
```json
{
  "stack": {
    "aws": { "...compiled stack JSON..." },
    "gcp": null,
    "local": { "...compiled stack JSON..." }
  },
  "gitMetadata": {
    "commitSha": "abc1234",
    "commitRef": "main",
    "commitMessage": "Add feature X"
  }
}
```

After storing the release, alien-manager sets `desired_release_id` on eligible deployments. See [Deployments — Desired vs Current Release](01-deployments.md#desired-vs-current-release).

**Auth:** Admin token only.

### Get Release

```
GET /v1/releases/{id}
```

Returns the full release record including `stack`.

### Get Latest Release

```
GET /v1/releases/latest
```

Returns the most recently created release.

## Deployment Groups

### Create Deployment Group

```
POST /v1/deployment-groups
```

```json
{
  "name": "production",
  "maxDeployments": 100
}
```

**Auth:** Admin token only.

### List Deployment Groups

```
GET /v1/deployment-groups
```

### Get Deployment Group

```
GET /v1/deployment-groups/{id}
```

### Create Deployment Group Token

```
POST /v1/deployment-groups/{id}/tokens
```

Generates a scoped token that can create deployments within this group.

**Response:**
```json
{
  "token": "ax_dg_abc123...",
  "deploymentGroupId": "dg_xxx"
}
```

**Auth:** Admin token only.

## Commands

See [Commands](03-commands.md) for the full protocol.

### Create Command

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

Large params (>150KB) are stored in blob storage.

**Auth:** Admin or deployment group token (own group).

### Get Command

```
GET /v1/commands/{id}
```

```json
{
  "id": "cmd_xxx",
  "deploymentId": "dp_xxx",
  "name": "generate-report",
  "state": "succeeded",
  "attempt": 1,
  "createdAt": "2025-01-01T00:00:00Z",
  "dispatchedAt": "2025-01-01T00:00:01Z",
  "completedAt": "2025-01-01T00:00:05Z",
  "requestSizeBytes": 128,
  "responseSizeBytes": 4096
}
```

### List Commands

```
GET /v1/commands
```

**Query params:**
- `deploymentId` — filter by deployment
- `limit` — max results

### Acquire Leases

```
POST /v1/commands/leases
```

Deployments call this periodically to pick up pending commands:

```json
{
  "deploymentId": "dp_xxx",
  "maxCommands": 5
}
```

Returns command envelopes with params. Matched commands transition to `dispatched` with a deadline timer.

**Auth:** Deployment token only.

### Submit Response

```
PUT /v1/commands/{id}/response
```

Deployment submits the command result.

**Auth:** Deployment token only.

### Release Lease

```
POST /v1/commands/leases/{id}/release
```

Release a lease without completing the command (e.g., on deployment shutdown). The command returns to `pending` for retry.

**Auth:** Deployment token only.

## Remote Bindings

See [Runtime — Remote Bindings](../04-runtime/01-bindings.md) for the full remote bindings concept.

### Resolve Credentials

```
POST /v1/resolve-credentials
```

Returns temporary cloud credentials for accessing a deployment's remote-access-enabled resources. Used by SaaS backends that need to interact with customer-side infrastructure (storage, KV, databases) at runtime.

**Request:**
```json
{
  "platform": "aws",
  "stackState": { "...deployment stack state..." }
}
```

**Response:**
```json
{
  "clientConfig": {
    "platform": "aws",
    "accessKeyId": "ASIA...",
    "secretAccessKey": "...",
    "sessionToken": "...",
    "region": "us-east-1"
  }
}
```

The server uses `CredentialResolver` to impersonate a service account in the target environment and returns temporary credentials scoped to the deployment's resources.

**Auth:** Admin or deployment group token.

## Artifact Registry

### Add Cross-Account Access

```
POST /v1/artifact-registry/repositories/{repo_id}/cross-account-access/add
```

Grants a remote account pull access to a repository in the artifact registry. Called during the `initial-setup` → `provisioning` deployment transition.

See [Releases — Cross-Account Registry Access](02-releases.md#cross-account-registry-access) for details on how this fits into the deployment lifecycle.

**Auth:** Admin token.

### Remove Cross-Account Access

```
POST /v1/artifact-registry/repositories/{repo_id}/cross-account-access/remove
```

Revokes a remote account's pull access to a repository. Called when a deployment is deleted.

**Auth:** Admin token.

## State Sync

Endpoints used by `alien deploy`, Operators, and other deployment tooling to coordinate deployment state. See [Deployments — The Deployment Lifecycle](01-deployments.md#the-deployment-lifecycle) for protocol details.

### Acquire

```
POST /v1/sync/acquire
```

Atomically locks deployments for processing. Returns deployment contexts with current state and configuration.

```json
{
  "session": "server-instance-uuid",
  "deploymentIds": ["dp_xxx"],
  "platforms": ["aws", "gcp"],
  "statuses": ["pending", "provisioning", "update-pending"],
  "limit": 10
}
```

### Reconcile

```
POST /v1/sync/reconcile
```

Writes new deployment state back to the database.

```json
{
  "deploymentId": "dp_xxx",
  "session": "session-uuid",
  "state": { "status": "provisioning", "resources": { } },
  "updateHeartbeat": false
}
```

### Release

```
POST /v1/sync/release
```

Clears `locked_by`. Always called after processing — even on failure — to prevent stuck locks.

### Operator Sync

```
POST /v1/sync
```

Pull-mode deployments call this to get their target state. Returns `{ target: { releaseInfo, config } }` or null if already up to date.

### Operator Initialize

```
POST /v1/initialize
```

First-time bootstrap. The Operator sends its token, and alien-manager creates a deployment record:
- Deployment group token → creates a new pull-mode deployment, returns `{ deploymentId, token }`
- Deployment token → returns `{ deploymentId }` (already exists)

## Telemetry

See [Telemetry](04-telemetry.md) for details.

### Ingest Logs

```
POST /v1/logs
```

Accepts OTLP log data (Protobuf). Scope determined from the Bearer token.

**Auth:** Deployment token.

### Ingest Traces

```
POST /v1/traces
```

Accepts OTLP trace data (Protobuf).

**Auth:** Deployment token.

### Ingest Metrics

```
POST /v1/metrics
```

Accepts OTLP metric data (Protobuf).

**Auth:** Deployment token.

## Identity

### Whoami

```
GET /v1/whoami
```

Returns the identity associated with the Bearer token.

```json
{
  "kind": "serviceAccount",
  "id": "dp_xxx",
  "scope": {
    "type": "deployment",
    "deploymentId": "dp_xxx"
  }
}
```

## Health

```
GET /health
```

Returns `{ "status": "healthy" }`. No auth required.
