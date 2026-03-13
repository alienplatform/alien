# Running alien-manager

## Quickstart

Deploy an Alien application to AWS using a self-hosted alien-manager.

### Start the server

```bash
docker run -d \
  -p 8080:8080 \
  -e AWS_ACCESS_KEY_ID=$AWS_ACCESS_KEY_ID \
  -e AWS_SECRET_ACCESS_KEY=$AWS_SECRET_ACCESS_KEY \
  -e AWS_REGION=us-east-1 \
  -e OTLP_ENDPOINT=http://your-grafana:4318 \
  -v alien-data:/data \
  ghcr.io/alienplatform/alien-manager:latest
```

On first run, alien-manager prints an admin API key to stdout:

```
Admin API key: ax_admin_abc123def456...
Save this key — it won't be shown again.
```

### Configure the CLI

```bash
export ALIEN_SERVER=http://localhost:8080
export ALIEN_API_KEY=ax_admin_abc123def456...
```

### Create a deployment group

```bash
alien deployment-groups create \
  --name production \
  --server $ALIEN_SERVER
```

Returns a deployment group token (`ax_dg_...`) for creating deployments within the group.

### Build, release, deploy

```bash
alien build --platform aws
alien release --server $ALIEN_SERVER
alien deploy \
  --server $ALIEN_SERVER \
  --token ax_dg_... \
  --platform aws \
  --name production
```

The deployment loop picks up the deployment, impersonates the configured AWS credentials, and runs `alien-deployment::step()` repeatedly until the deployment reaches `running`.

### Check status

```bash
alien deployments ls --server $ALIEN_SERVER
```

```
NAME        STATUS    PLATFORM  RELEASE
production  running   aws       rel_abc123
```

### Send a command

```bash
alien command invoke \
  --server $ALIEN_SERVER \
  --deployment production \
  --command my-command \
  --params '{"key": "value"}'
```

### Ship an update

Push a new release and deployments update automatically:

```bash
alien build --platform aws
alien release --server $ALIEN_SERVER
```

### Pull model (Kubernetes)

For environments where alien-manager can't impersonate credentials directly, install the Operator:

```bash
helm install alien-operator alien/operator \
  --set syncUrl=$ALIEN_SERVER \
  --set token=ax_dg_... \
  --set platform=kubernetes
```

The Operator polls alien-manager for updates and deploys locally using in-cluster credentials.

## Configuration

alien-manager reads configuration from environment variables.

### Database

```bash
DATA_DIR=/data  # Directory for SQLite database and local state (default: /data in Docker, .alien/ in dev mode)
```

The database file lives at `{DATA_DIR}/alien.db`. Migrations run automatically on startup — idempotent, safe to run on every restart. The `SqliteDeploymentStore` uses `sea-query` to build DDL with `IF NOT EXISTS` guards, plus `ALTER TABLE` statements that silently ignore "column already exists" errors.

### Server

```bash
HOST=0.0.0.0          # Listen address (default: 0.0.0.0)
PORT=8080              # Listen port (default: 8080)
BASE_URL=https://alien.example.com  # Public URL (for command response URLs, deployment info)
```

If `BASE_URL` is not set, alien-manager uses `http://{HOST}:{PORT}`.

### Cloud credentials

Credentials for the deployment loop to impersonate a service account in the target remote environment. See [Deployments — Credential Impersonation](01-deployments.md#credential-impersonation).

```bash
# AWS
AWS_ACCESS_KEY_ID=AKIA...
AWS_SECRET_ACCESS_KEY=...
AWS_REGION=us-east-1
# Or use a role
AWS_ROLE_ARN=arn:aws:iam::123456789:role/alien-manager

# GCP
GOOGLE_APPLICATION_CREDENTIALS=/path/to/service-account.json
# Or use workload identity (no env vars needed in GKE)

# Azure
AZURE_SUBSCRIPTION_ID=...
AZURE_TENANT_ID=...
AZURE_CLIENT_ID=...
AZURE_CLIENT_SECRET=...
```

Multiple platforms can be configured simultaneously.

### Telemetry

```bash
OTLP_ENDPOINT=http://grafana-alloy:4318  # Forward all telemetry here
```

If not set, telemetry is accepted but discarded. See [Telemetry](04-telemetry.md).

### Deployment loop

```bash
DEPLOYMENT_INTERVAL=10     # Seconds between deployment loop iterations (default: 10)
HEARTBEAT_INTERVAL=60      # Seconds between heartbeat checks (default: 60)
TARGETS=aws,gcp            # Comma-separated platforms to deploy (default: all configured)
```

### Authentication

On first startup, alien-manager generates an admin API key and prints it to stdout. The raw token is never stored — only its SHA-256 hash. See [Authentication](05-auth.md) for token types, format, and scope enforcement.

### Full example

```bash
docker run -p 8080:8080 \
  -e BASE_URL=https://alien.example.com \
  -e AWS_ACCESS_KEY_ID=AKIA... \
  -e AWS_SECRET_ACCESS_KEY=... \
  -e AWS_REGION=us-east-1 \
  -e GOOGLE_APPLICATION_CREDENTIALS=/creds/gcp.json \
  -e OTLP_ENDPOINT=http://grafana:4318 \
  -e TARGETS=aws,gcp \
  -v /creds:/creds:ro \
  -v alien-data:/data \
  ghcr.io/alienplatform/alien-manager:latest
```

## Kubernetes (Helm)

alien-manager provides a Helm chart for Kubernetes deployments.

### Install

```bash
helm install alien-manager oci://ghcr.io/aliendotdev/charts/alien-manager \
  --namespace alien \
  --create-namespace \
  --values values.yaml
```

### values.yaml

```yaml
# Public URL for the server (used in command response URLs, deployment info)
baseUrl: https://alien.example.com

# Persistent storage for SQLite database
storage:
  size: 10Gi
  storageClassName: gp3  # Uses cluster default if not specified

# Cloud credentials for the deployment loop.
# The server impersonates these to provision resources in target environments.
credentials:
  # Option 1: Kubernetes ServiceAccount with cloud identity annotations.
  # The chart creates a ServiceAccount with these annotations — use this
  # for IRSA (EKS), Workload Identity (GKE), or Managed Identity (AKS).
  serviceAccount:
    annotations: {}
      # AWS/EKS (IRSA):
      # eks.amazonaws.com/role-arn: arn:aws:iam::123456789:role/alien-manager
      #
      # GCP/GKE (Workload Identity):
      # iam.gke.io/gcp-sa: alien-manager@project.iam.gserviceaccount.com
      #
      # Azure/AKS (Workload Identity):
      # azure.workload.identity/client-id: "YOUR_CLIENT_ID"
    labels: {}
      # Azure requires:
      # azure.workload.identity/use: "true"

  # Option 2: Static credentials via environment variables.
  # Less secure than ServiceAccount-based identity — prefer option 1 when possible.
  env: {}
    # AWS_ACCESS_KEY_ID: AKIA...
    # AWS_SECRET_ACCESS_KEY: ...
    # AWS_REGION: us-east-1

  # Option 3: Reference an existing Kubernetes Secret containing credential env vars.
  # existingSecret: alien-manager-credentials

# Deployment loop
deploymentLoop:
  interval: 10          # Seconds between iterations (default: 10)
  heartbeatInterval: 60 # Seconds between heartbeat checks (default: 60)
  targets: []           # Platforms to deploy: [aws, gcp, azure, kubernetes]

# Telemetry forwarding
telemetry:
  otlpEndpoint: ""  # e.g., http://grafana-alloy:4318

# Artifact registry binding (for cross-account image access)
artifactRegistry:
  enabled: false
  # platform: aws
  # region: us-east-1
```

### What the chart creates

- **Deployment** — single-replica alien-manager pod
- **Service** — ClusterIP on port 8080
- **PersistentVolumeClaim** — SQLite database storage
- **ServiceAccount** — with cloud identity annotations for credential resolution
- **Secret** — admin API key (generated on first startup, stored as a Kubernetes Secret for persistence across restarts)

### Example: EKS with IRSA

```yaml
baseUrl: https://alien.example.com

credentials:
  serviceAccount:
    annotations:
      eks.amazonaws.com/role-arn: arn:aws:iam::123456789:role/alien-manager

deploymentLoop:
  targets: [aws]

telemetry:
  otlpEndpoint: http://grafana-alloy.monitoring:4318

artifactRegistry:
  enabled: true
  platform: aws
  region: us-east-1
```

### Example: GKE with Workload Identity

```yaml
baseUrl: https://alien.example.com

credentials:
  serviceAccount:
    annotations:
      iam.gke.io/gcp-sa: alien-manager@my-project.iam.gserviceaccount.com

deploymentLoop:
  targets: [gcp]

telemetry:
  otlpEndpoint: http://grafana-alloy.monitoring:4318

artifactRegistry:
  enabled: true
  platform: gcp
```
