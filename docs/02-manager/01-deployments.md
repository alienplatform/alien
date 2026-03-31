# Deployments

How code goes from your machine to running in a customer's cloud. This doc covers the deployment data model, the deployment lifecycle, push and pull execution, and credential impersonation.

For the high-level push vs pull narrative, read [alien-manager — How Deployments Work](00-overview.md#how-deployments-work) first.

## The Deployment Record

```sql
CREATE TABLE deployments (
  id                  TEXT PRIMARY KEY,     -- "dp_" prefix, nanoid
  name                TEXT NOT NULL,
  deployment_group_id TEXT NOT NULL,
  platform            TEXT NOT NULL,        -- "aws" | "gcp" | "azure" | "kubernetes" | "local"
  status              TEXT NOT NULL,        -- see Status Lifecycle below
  stack_settings      TEXT,                 -- JSON: StackSettings (network, domains, etc.)
  stack_state         TEXT,                 -- JSON: DeploymentState from alien-deployment
  environment_info    TEXT,                 -- JSON: target environment metadata
  runtime_metadata    TEXT,                 -- JSON: runtime-reported metadata
  current_release_id  TEXT,                 -- release currently deployed
  desired_release_id  TEXT,                 -- release the deployment should converge to
  retry_requested     INTEGER DEFAULT 0,    -- 1 = deployment loop should retry
  error               TEXT,                 -- JSON: AlienError if in a failed state
  created_at          TEXT DEFAULT CURRENT_TIMESTAMP,
  updated_at          TEXT
)
```

### Status lifecycle

```
pending → initial-setup → provisioning → running
                                           ↓
running → update-pending → updating → running
                                           ↓
running → delete-pending → deleting → deleted (removed from DB)

Any status → *-failed (on error)
*-failed + retry_requested → retries from appropriate phase
```

| Status | Meaning |
|--------|---------|
| `pending` | Created, waiting for deployment loop |
| `initial-setup` | Deployment loop acquired, creating base resources |
| `provisioning` | Provisioning cloud resources (IAM, functions, storage) |
| `running` | Fully deployed and healthy |
| `update-pending` | New release available, waiting for deployment loop |
| `updating` | Deploying new release |
| `delete-pending` | Deletion requested, waiting for deployment loop |
| `deleting` | Tearing down cloud resources |
| `*-failed` | Error in the corresponding phase |

### Desired vs current release

When a new release is created, alien-manager sets `desired_release_id` on eligible deployments. The deployment loop converges `current_release_id` toward `desired_release_id`. When they match, the deployment is up to date.

Update triggers (on new release):
- `pending` with no desired release → sets `desired_release_id`
- `running` with no desired release → sets `update-pending` + `desired_release_id`
- `*-failed` → sets `desired_release_id` + `retry_requested`

## Deployment Groups

A logical grouping of deployments. Controls how many deployments can be created (fleet management) and provides scoped tokens for deployment creation.

```sql
CREATE TABLE deployment_groups (
  id                TEXT PRIMARY KEY,   -- "dg_" prefix, nanoid
  name              TEXT NOT NULL,
  max_deployments   INTEGER DEFAULT 100,
  deployment_count  INTEGER DEFAULT 0,
  created_at        TEXT DEFAULT CURRENT_TIMESTAMP
)
```

Use cases:
- **Single deployment**: one deployment group with `max_deployments=1`
- **Multiple environments for one customer**: one deployment group; staging, production, and eu are separate deployments within it
- **Fleet**: one deployment group per customer, one deployment per device or endpoint

## The Deployment Lifecycle

### Initial setup

An admin in the remote environment initiates the first deployment:

```bash
alien deploy --token ax_dg_... --name production --platform aws
```

This calls alien-manager's sync APIs and runs a loop:

1. **Acquire** — `POST /v1/sync/acquire` locks the deployment
2. **Step** — calls `alien-deployment::step()` with the admin's cloud credentials
3. **Reconcile** — `POST /v1/sync/reconcile` writes the new state back
4. Repeat until `running`
5. **Release** — `POST /v1/sync/release` unlocks the deployment (always, even on failure)

During initial setup, `step()` provisions frozen resources first (IAM roles, storage buckets) with elevated permissions, then live resources (functions, containers) with minimal permissions.

`alien deploy` is one way to trigger this. Other methods:

- **White-labeled CLI** — the platform generates a project-specific CLI for customers
- **CloudFormation template** — a one-click template that provisions the necessary infrastructure
- **Terraform provider** — runs `step()` inside `terraform apply`
- **"Login with Google" button** — GCP OAuth flow that creates the deployment via the dashboard
- **Agent** — a controller in the remote environment's Kubernetes cluster (see Pull Model below)

All call the same sync APIs.

### The step function

`alien_deployment::step()` is a pure, stateless state machine. Given:
- Current `DeploymentState` (from the database)
- `DeploymentConfig` (release info, stack settings)
- `ClientConfig` (credentials for the target environment)
- A `BindingsProvider` (for accessing cloud APIs)

It returns:
- New `DeploymentState` (or unchanged if waiting)
- Optional `suggested_delay_ms` (how long to wait before the next step)
- Optional error

The deployment loop calls `step()` repeatedly until the deployment reaches a stable state or the step suggests a long delay (>500ms). This allows fast transitions (e.g., `pending` → `initial-setup` → `provisioning`) to happen in a single loop iteration. Maximum 100 steps per iteration to prevent indefinite processing.

### Loop contract

`alien-deployment` defines a canonical loop contract that all callers use to interpret deployment status after each step:

- `classify_status(status, operation)` maps the current deployment status to a terminal outcome (`Success`, `Failure`, `Neutral`) or returns `None` if the loop should continue.
- For **Deploy** operations: `Running` is success, `Provisioning`/`Updating` is handoff (neutral — another actor takes over), and any `*Failed` status is failure.
- For **Delete** operations: `Deleted` is success, any `*Failed` status is failure. Non-delete statuses like `Running` are non-terminal (the loop continues stepping).
- The shared runner (`alien-deployment::runner`) wraps this contract into a step loop with a configurable budget. If the budget is exceeded without reaching a terminal state, the outcome is `Failure`.

The manager loop, `alien-deploy-cli`, and `alien-agent` all use this contract. Loop outcome interpretation is centralized — callers don't implement their own terminal-state detection.

### Push model

The deployment loop is alien-manager's background process for push-model deployments. It runs as a `tokio::spawn` task on a configurable interval:

```
1. Poll: which deployments need work?
2. Impersonate: get credentials for target env
3. Step: call alien-deployment::step()
4. Reconcile: write new state back to DB

Repeat every DEPLOYMENT_INTERVAL seconds (default: 10, dev mode: 1)
```

The loop selects push-model deployments with these statuses:

| Status | Action |
|--------|--------|
| `pending` | Start initial deployment |
| `initial-setup` | Continue setup |
| `provisioning` | Continue provisioning |
| `update-pending` | Start update |
| `updating` | Continue update |
| `delete-pending` | Start teardown |
| `deleting` | Continue teardown |
| `*-failed` with `retry_requested` | Retry from appropriate phase |

After each step, the loop writes the new deployment state back via `reconcile()`. When `current_release_id` matches `desired_release_id`, it clears `desired_release_id` — the deployment is up to date. When status reaches `deleted`, the deployment is removed from the database.

A separate heartbeat loop (default 60s) polls `running` deployments and updates their heartbeat timestamp. This enables stale deployment detection.

The standalone alien-manager is single-instance. The deployment loop processes deployments sequentially within each iteration — no locking needed. The `DeploymentStore` trait supports a `locked_by` mechanism for platform-mode deployments where multiple manager instances run concurrently.

### Pull model

For environments where alien-manager can't call cloud APIs directly — Kubernetes clusters, airgapped networks, or organizations that don't allow cross-account access.

Instead of granting alien-manager credentials, the admin installs an **Agent** that lives inside the remote environment and polls alien-manager for updates:

```
alien-manager                     Agent (in customer's K8s)
     │                                │
     │◀── POST /v1/initialize ────────│  "Here's my token, create a deployment"
     │── { deploymentId, token } ────▶│
     │                                │
     │◀── POST /v1/sync ─────────────│  "Here's my current state"
     │── { target } ─────────────────▶│  "Here's what you should deploy"
     │                                │
     │   (Agent runs step() locally    │
     │    with in-cluster credentials) │
     │                                │
     │◀── POST /v1/sync/reconcile ───│  "I've reached this state"
     │                                │
```

The Agent:
1. Calls `POST /v1/initialize` on first startup to create a deployment record
2. Periodically calls `POST /v1/sync` to check for updates
3. Runs `alien-deployment::step()` locally with its own credentials
4. Reports state back via `POST /v1/sync/reconcile`

Same `step()` function, same state machine. The Agent runs in the remote environment and uses local credentials. alien-manager stores the target state and serves it on request.

### Credential impersonation

In push mode, alien-manager calls cloud APIs in the remote environment. It reads its own credentials from environment variables via `ClientConfig::from_std_env()`:

```bash
# AWS — alien-manager's own identity
AWS_ACCESS_KEY_ID=AKIA...
AWS_SECRET_ACCESS_KEY=...
AWS_REGION=us-east-1
# Or use a role (e.g. IRSA on EKS)
AWS_ROLE_ARN=arn:aws:iam::123456789:role/alien-manager

# GCP — alien-manager's own identity
GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
# Or use workload identity (no env vars needed in GKE)

# Azure — alien-manager's own identity
AZURE_SUBSCRIPTION_ID=...
AZURE_TENANT_ID=...
AZURE_CLIENT_ID=...
AZURE_CLIENT_SECRET=...
```

These are alien-manager's own credentials — its base identity, not the remote environment's credentials.

For **single-account setups** (deploying to your own cloud), these credentials act directly on the target account. No additional setup needed.

For **cross-account setups** (deploying to a customer's cloud), alien-manager uses these base credentials to impersonate a role in the remote environment. This requires:
- AWS: `sts:AssumeRole` permission for a role in the target account
- GCP: `roles/iam.serviceAccountTokenCreator` for a service account in the target project
- Azure: a service principal with role assignments in the target subscription

The target role or service account is created during initial setup (`alien deploy`) and stored in the deployment's `RemoteStackManagement` stack state. The deployment loop reads it and calls `ClientConfig::impersonate()` before running `step()`.

Multiple platforms can be configured simultaneously. The loop uses credentials matching each deployment's target platform.

In dev mode, alien-manager uses `ClientConfig::Local { state_directory }` — no cloud credentials. The deployment state machine runs everything locally via Docker.

## Updates and Deletion

**Updates:** When a new release is created, alien-manager sets `desired_release_id` on eligible deployments. Deployments in `running` status transition to `update-pending`. The deployment loop (push) or Agent (pull) picks this up and runs `step()` until `current_release_id` matches `desired_release_id`.

**Redeployment:** `POST /v1/deployments/{id}/redeploy` forces a re-provision of a running deployment with the same release. Sets status to `update-pending` so the deployment loop re-runs `step()`. Useful when infrastructure changes happened outside of Alien.

**Deletion:** `DELETE /v1/deployments/{id}` sets status to `delete-pending`. For push-model deployments, the manager loop skips deletion phases (`DeletePending`, `Deleting`, `DeleteFailed`) since these require target-environment credentials that only the developer's machine has. Instead, `alien-deploy-cli` drives deletion locally via `push_deletion`, which acquires the deployment, runs the delete step loop with `LoopOperation::Delete`, reconciles state, and releases the lock. For pull-model deployments, the agent handles deletion in-environment. The deployment record is removed when status reaches `deleted`.

**Errors:** Any phase can transition to `*-failed` with a structured error in the `error` field. `POST /v1/deployments/{id}/retry` sets `retry_requested = true`, and the deployment loop retries from the appropriate phase.

## Post-Reconcile Update Detection

A race condition exists when a new release arrives while a deployment is already updating. When release B is created while the deployment is updating to release A, `create_release` skips setting `desired_release_id` because the deployment already has a desired release (A). After A's update completes, `reconcile()` clears `desired_release_id` — the deployment sits at `running` with release A, unaware of B.

To handle this, after `reconcile()` clears `desired_release_id` (i.e., `current_release_id` now matches `desired_release_id`), it checks `get_latest_release()`. If the latest release is newer than `current_release_id`, it immediately sets `desired_release_id` to the latest release and transitions the deployment to `update-pending`:

```
reconcile(deployment, new_state):
    if new_state.current_release_id == deployment.desired_release_id:
        clear desired_release_id
        latest = get_latest_release()
        if latest.id != current_release_id:
            set desired_release_id = latest.id
            set status = update-pending
```

This check runs inline during reconciliation — no background polling loop needed. Since the OSS server is single-instance, this covers all cases without interval tuning or coordination overhead.

## Environment Variables Injected into Containers

The deployment loop injects these environment variables into each deployment's containers:

```bash
ALIEN_DEPLOYMENT_ID=dp_xxx
OTEL_EXPORTER_OTLP_LOGS_ENDPOINT=http://server:8080/v1/logs
OTEL_EXPORTER_OTLP_TRACES_ENDPOINT=http://server:8080/v1/traces
OTEL_EXPORTER_OTLP_METRICS_ENDPOINT=http://server:8080/v1/metrics
OTEL_EXPORTER_OTLP_HEADERS=authorization=Bearer ax_deploy_...
ALIEN_COMMANDS_POLLING_ENABLED=true
ALIEN_COMMANDS_POLLING_URL=http://server:8080/v1/commands/leases
ALIEN_TOKEN=ax_deploy_...
```

The `OTEL_*` variables are standard OpenTelemetry environment variables — any OTLP-compatible SDK or collector picks them up automatically.

Plus any user-supplied environment variables from the deployment's configuration.
