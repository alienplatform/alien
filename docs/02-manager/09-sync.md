# Sync Protocol

The sync protocol is how pull-mode agents communicate with alien-manager. A single bidirectional endpoint — `POST /v1/sync` — handles both state reporting and target delivery.

## The Exchange

The agent calls `POST /v1/sync` on a periodic interval (default: 10 seconds):

```json
{
  "deploymentId": "dp_xxx",
  "currentState": { ... }
}
```

The manager responds with the target the agent should converge toward:

```json
{
  "target": {
    "releaseInfo": { "releaseId": "rel_xxx", "stack": { ... } },
    "config": { "stackSettings": { ... }, "environmentVariables": { ... } }
  },
  "commandsUrl": "https://manager.example.com/v1"
}
```

When the deployment is already up to date, `target` is `null` — the agent has nothing to do.

## State Reporting

The `currentState` field is optional. When present, the manager persists it to the deployment record. This is how agents propagate status changes (`Pending` → `Running`, resource outputs, environment info) back to the manager so API consumers can observe deployment progress.

The manager calls the same `reconcile()` path used by push-mode state updates. After reconciling, it also triggers post-reconcile actions like cross-account registry access grants.

When `currentState` is absent, the agent is asking for its target without reporting any progress. This happens on the first sync call before the agent has run any steps.

## Target Delivery

The manager returns a `TargetDeployment` when the deployment's `desired_release_id` differs from its `current_release_id`. The target contains:

- **Release info** — release ID, stack definition
- **Deployment config** — stack settings, environment variables, feature flags

The agent uses this to run `alien-deployment::step()` locally with its own in-environment credentials.

## Push-Mode Sync (Acquire / Reconcile / Release)

Push-mode deployments use a different set of endpoints. The deployment loop acquires a lock, runs steps, and releases:

```
POST /v1/sync/acquire     — lock deployments for processing
POST /v1/sync/reconcile   — write state after each step
POST /v1/sync/release     — release the lock
```

These require admin-level authentication and support batch processing. See [Deployments — Push model](01-deployments.md#push-model) for the full lifecycle.

## Authentication

`POST /v1/sync` accepts deployment tokens (`ax_deploy_...`). The token must match the deployment ID in the request — agents can only sync their own deployment.

The push-mode endpoints (`acquire`, `reconcile`, `release`) require admin tokens, except `reconcile` and `release` which also accept the matching deployment token for pull-mode reconciliation.

## Commands URL

Every sync response includes `commandsUrl` — the base URL for the commands lease API. Cloud-deployed functions use this to poll for pending commands at `{commandsUrl}/commands/leases`. When absent, the agent falls back to deriving the URL from its sync endpoint.

## Initialization

Before the first sync, a new agent calls `POST /v1/initialize`:

```json
{
  "name": "production",
  "platform": "kubernetes"
}
```

The manager creates a deployment record and returns a deployment-scoped token:

```json
{
  "deploymentId": "dp_xxx",
  "token": "ax_deploy_..."
}
```

The agent stores this token and uses it for all subsequent sync calls.

## Implementation

Types: `alien_core::sync::{SyncRequest, SyncResponse, TargetDeployment}`

Routes: `alien-manager/src/routes/sync.rs`
