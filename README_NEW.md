# Alien

Ship software to remote environments you don't control — and keep it fully managed.

Your customers want your software in *their* cloud. Alien deploys it there and keeps it updated, monitored, and under your control. No more shipping Docker images and hoping for the best.

Works with **AWS**, **GCP**, **Azure**, **Kubernetes**, and **local machines**.

---

## How It Works

Deploying to remote environments happens in two phases:

1. **One-time setup** — the customer's admin creates infrastructure and grants you limited access.
2. **Ongoing** — you push code, updates, and teardowns without the admin involved.

Two deployment models:

- **Push** (AWS, GCP, Azure) — the manager calls cloud APIs directly to provision and update resources.
- **Pull** (Local, Kubernetes) — an agent runs in the remote environment, polls the manager for updates, and deploys locally.

In both cases, the software is fully managed even though it lives in a remote environment.

---

## Quick Start

### 1. Install the CLI

```bash
curl -fsSL https://alien.dev/install | bash
```

### 2. Define your app

Create `alien.ts` in your project:

```typescript
import * as alien from "@alienplatform/core"

const api = new alien.Function("api")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .ingress("public")
  .permissions("execution")
  .build()

export default new alien.Stack("my-app")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
```

No `package.json` required — the CLI auto-resolves `@alienplatform/core`.

### 3. Start the manager

```bash
alien serve
```

On first run, this prints an admin token. Save it:

```
Generated admin token (save this securely):
  am_abc123def456...

Set it as ALIEN_API_KEY when using the CLI:
  export ALIEN_SERVER=http://localhost:8080
  export ALIEN_API_KEY=am_abc123def456...
```

The manager stores everything in SQLite — no external database needed.

### 4. Push a release

```bash
export ALIEN_SERVER=http://localhost:8080
export ALIEN_API_KEY=am_abc123def456...

alien release
```

This builds your app and pushes the release to the manager. No separate `alien build` step needed.

### 5. Onboard a remote environment

```bash
alien onboard production-fleet --project default
```

This creates a *deployment group* and outputs a deploy link:

```
Deployment group 'production-fleet' created successfully

Deployment Link:
   http://localhost:8080/deploy#token=dg_abc123...

Share this link with the admin of the remote environment.
```

### 6. Deploy (admin side)

The admin clicks the deploy link, selects their platform, and runs:

```bash
# Install the deploy CLI
curl -fsSL http://your-manager-url/install | bash

# Deploy
alien-deploy up \
  --token dg_abc123... \
  --platform aws \
  --manager-url http://your-manager-url
```

That's it. The software is now running in the remote environment and receiving updates automatically.

---

## Try It Locally (from source)

Run the full flow on your machine using the included example app. Requires [Rust](https://rustup.rs/), [Node.js 22+](https://nodejs.org/), [pnpm](https://pnpm.io/), and [Bun](https://bun.sh/).

### 1. Build the binaries

```bash
git clone https://github.com/alienplatform/alien.git && cd alien

# Build the CLI, deploy CLI, and agent
cargo build -p alien-cli -p alien-deploy-cli -p alien-agent

# Install example app dependencies
cd examples && pnpm install && cd ..
```

### 2. Start the manager

In a dedicated terminal:

```bash
./target/debug/alien serve --port 8090
```

On first run this prints an admin token and a quick-start snippet. Copy the two `export` lines.

### 3. Build and release

In a second terminal, set the env vars printed by `alien serve`:

```bash
export ALIEN_SERVER=http://localhost:8090
export ALIEN_API_KEY=<token from step 2>

cd examples/minimal-cloud-agent

# Build
../../target/debug/alien build --platform local --no-tui

# Release (pushes the build to the manager)
../../target/debug/alien release --platform local --yes --no-tui
```

### 4. Create a deployment group

```bash
../../target/debug/alien onboard my-fleet
```

This prints a deploy link and a deployment group token (`dg_...`). Copy the token.

### 5. Deploy locally

In a third terminal (keep the manager running):

```bash
export ALIEN_AGENT_BINARY=$PWD/../../target/debug/alien-agent

../../target/debug/alien-deploy up \
  --token <dg_token from step 4> \
  --platform local \
  --manager-url http://localhost:8090 \
  --foreground
```

The `--foreground` flag runs the agent inline instead of installing an OS service — great for testing. You'll see the deployment progress through Pending → InitialSetup → Provisioning → Running, and the function starts accepting requests.

Press Ctrl+C to stop.

> **Production mode:** Without `--foreground`, the agent is installed as a system service (systemd/launchd). This requires `sudo`.

### 6. Verify

```bash
# Check deployment status
curl -s -H "Authorization: Bearer $ALIEN_API_KEY" http://localhost:8090/v1/deployments | jq '.[].status'

# Hit the deployed app
curl -s http://localhost:<port>/health
```

The function port is shown in the deployment's `stackState.resources.agent.outputs.url`.

### 7. Invoke a command

Once the deployment is running, invoke the echo command via the Commands API:

```bash
# Get the deployment ID
DEPLOYMENT_ID=$(curl -s -H "Authorization: Bearer $ALIEN_API_KEY" http://localhost:8090/v1/deployments | jq -r '.[0].id')

# Invoke the echo command
COMMAND_ID=$(curl -s -X POST http://localhost:8090/v1/commands \
  -H "Authorization: Bearer $ALIEN_API_KEY" \
  -H "Content-Type: application/json" \
  -d "{\"deploymentId\": \"$DEPLOYMENT_ID\", \"command\": \"echo\", \"params\": {\"mode\": \"inline\", \"inlineBase64\": \"$(echo -n '{\"message\":\"hello from commands!\"}' | base64)\"}}" | jq -r '.commandId')

# Poll for the response
curl -s -H "Authorization: Bearer $ALIEN_API_KEY" http://localhost:8090/v1/commands/$COMMAND_ID | jq
```

---

## Developer Guide

### The `alien.ts` Config

The config file defines everything that runs in the remote environment:

```typescript
import * as alien from "@alienplatform/core"

// A serverless function
const api = new alien.Function("api")
  .code({ type: "source", src: "./api", toolchain: { type: "typescript" } })
  .ingress("public")
  .permissions("execution")
  .build()

// A long-running container
const worker = new alien.Container("worker")
  .image("./worker/Dockerfile")
  .resources({ cpu: "0.5", memory: "512Mi" })
  .build()

export default new alien.Stack("my-app")
  .add(api, "live")
  .add(worker, "live")
  .permissions({
    profiles: {
      execution: {
        storage: ["read", "write"],
        kv: ["read", "write"],
      },
    },
  })
  .build()
```

### Cloud-Agnostic SDK

The SDK abstracts away cloud differences. Same code runs on every platform:

| Abstraction | AWS | GCP | Azure | Local |
|-------------|-----|-----|-------|-------|
| `storage()` | S3 | Cloud Storage | Blob Storage | Filesystem |
| `ai()` | Bedrock | Vertex AI | Azure OpenAI | Local model |
| `queue()` | SQS | Pub/Sub | Service Bus | In-memory |
| `kv()` | DynamoDB | Firestore | Table Storage | SQLite |

The SDK is optional. Use any existing libraries or call cloud APIs directly.

### Remote Commands

Invoke code on deployments. Zero inbound networking. Zero open ports.

**Handler side** (in the deployed function):

```typescript
import { command } from "@alienplatform/bindings"

command("generate-report", async ({ startDate, endDate }) => {
  const data = await fetchData(startDate, endDate)
  return { report: aggregate(data) }
})
```

**Invocation side** (from your app/script):

```typescript
import { CommandsClient } from "@alienplatform/commands-client"

const commands = new CommandsClient({
  managerUrl: "https://your-manager-url",
  deploymentId: "deployment_123",
  token: "your_api_key",
})

const result = await commands.invoke("generate-report", {
  startDate: "2025-01-01",
})
```

### Releasing Updates

Push a new release whenever your code changes:

```bash
alien release
```

All deployments in the push model update automatically. Pull-model deployments pick up the update on the next sync cycle (every 30 seconds by default).

### Managing Deployments

```bash
# List all deployments
alien deployments ls

# Check a specific deployment
alien deployments get <deployment-id>

# Destroy a deployment
alien destroy <deployment-id>
```

### Local Development

```bash
alien dev
```

Starts a local manager with SQLite, builds your stack, and deploys everything with Docker. No cloud credentials needed. Hot-reloads on code changes.

---

## Admin Guide

You've received a deploy link from a software provider. Here's how to deploy.

### Install alien-deploy

```bash
curl -fsSL https://your-manager-url/install | bash
```

### Deploy (Push Model — AWS/GCP/Azure)

The push model is used when the manager can reach cloud APIs. You provide initial credentials, and the manager takes over:

```bash
alien-deploy up \
  --token dg_abc123... \
  --platform aws \
  --manager-url https://your-manager-url
```

This creates a cross-account IAM role (or equivalent) and returns. The manager continues provisioning in the background.

Ensure your cloud credentials are available in the environment (`AWS_ACCESS_KEY_ID`, etc.).

### Deploy (Pull Model — Local/Kubernetes)

The pull model installs an agent that runs continuously in your environment:

```bash
alien-deploy up \
  --token dg_abc123... \
  --platform local \
  --manager-url https://your-manager-url
```

This downloads and installs `alien-agent` as a system service.

### Manage Deployments

```bash
# Check deployment status
alien-deploy status

# List all tracked deployments
alien-deploy list

# Tear down a deployment
alien-deploy down
```

### Manage the Agent Service

```bash
# Install agent as OS service (systemd/launchd/Windows)
alien-deploy agent install \
  --sync-url https://your-manager-url \
  --sync-token dg_abc123... \
  --platform local

# Start/stop/status/uninstall
alien-deploy agent start
alien-deploy agent stop
alien-deploy agent status
alien-deploy agent uninstall
```

---

## Kubernetes Guide

### Helm Chart

Deploy the agent to Kubernetes using the Helm chart:

```bash
helm install alien-agent oci://ghcr.io/alienplatform/charts/alien-agent \
  --set syncUrl=https://your-manager-url \
  --set syncToken=dg_abc123... \
  --set encryptionKey=$(openssl rand -hex 32) \
  --set namespace=default
```

The agent runs as a single-replica Deployment, polls the manager for updates, and creates Pods, Services, and other resources in the target namespace.

### Values

| Value | Description | Default |
|-------|-------------|---------|
| `syncUrl` | Manager URL | (required) |
| `syncToken` | Deployment group or deployment token | (required) |
| `encryptionKey` | 64-char hex key for state encryption | (required) |
| `platform` | Target platform | `kubernetes` |
| `namespace` | Namespace for managed resources | `""` |
| `persistence.enabled` | Enable PVC for state persistence | `true` |
| `persistence.size` | PVC size | `1Gi` |
| `image.tag` | Agent image tag | `latest` |

---

## Architecture

```
Developer                          Remote Environment
─────────                          ──────────────────

alien.ts                        ┌─ Push Model ──────────┐
  │                             │                       │
  ▼                             │  Manager calls cloud  │
alien release ──► Manager ──────┤  APIs directly        │
                  (SQLite)      │  (AWS/GCP/Azure)      │
                    │           └───────────────────────┘
                    │
                    │           ┌─ Pull Model ──────────┐
                    └──────────►│                       │
                                │  Agent polls manager  │
                                │  every 30s, deploys   │
                                │  locally              │
                                │  (Local/Kubernetes)   │
                                └───────────────────────┘
```

### Manager

The manager is the control plane. It stores releases, manages deployments, dispatches commands, and collects telemetry.

- **Push model**: The manager holds cloud credentials and provisions resources directly via cloud APIs.
- **Pull model**: The agent calls `/v1/sync` every 30 seconds. The manager responds with the target state. The agent runs `alien-deployment::step()` locally.
- **Embeddable**: The manager is a Rust library with a builder API. Swap providers for storage, credentials, telemetry, and auth to embed it in your own hosting service.

### Agent

The agent (`alien-agent`) runs in the remote environment for pull-model deployments:

- Syncs with the manager every 30 seconds
- Runs deployment steps locally using in-cluster or local credentials
- Collects and forwards telemetry (logs, metrics, traces)
- Supports airgapped/offline operation with encrypted local state
- Runs as a system service (systemd, launchd, Windows SCM) or Kubernetes Deployment

### Crates

| Crate | Description |
|-------|-------------|
| `alien-cli` | Developer CLI (`alien serve`, `alien release`, `alien dev`, etc.) |
| `alien-manager` | Control plane server (embeddable library + standalone binary) |
| `alien-agent` | Pull-model deployment agent |
| `alien-deploy-cli` | Remote environment CLI for admins |
| `alien-deployment` | Core deployment loop (`step()` function) |
| `alien-core` | Shared types (Stack, Platform, DeploymentState, etc.) |
| `alien-build` | Build system (TypeScript, Rust, containers) |
| `alien-runtime` | Serverless function runtime |
| `alien-infra` | Infrastructure provisioning (IAM, networking, compute) |
| `alien-bindings` | Cloud-agnostic resource bindings (storage, KV, queues) |

---

## Self-Hosting

### Running the Manager

```bash
# Option 1: CLI
alien serve --port 8080

# Option 2: Docker
docker run -d \
  -p 8080:8080 \
  -v alien-data:/app/.alien \
  ghcr.io/alienplatform/alien-server:latest
```

The manager needs to be accessible from remote environments. Set `BASE_URL` to the public URL:

```bash
alien serve --port 8080
# Set BASE_URL=https://manager.yourdomain.com
```

### Configuration

| Environment Variable | Description | Default |
|---------------------|-------------|---------|
| `PORT` | Server port | `8080` |
| `ALIEN_DB_PATH` | SQLite database path | `alien-manager.db` |
| `ALIEN_STATE_DIR` | State directory | `.alien-manager` |
| `BASE_URL` | Public URL (for deploy page, install script) | `http://localhost:{port}` |
| `OTLP_ENDPOINT` | Forward telemetry to this OTLP endpoint | (disabled) |
| `ALIEN_RELEASES_URL` | Base URL for binary downloads (install script, agent) | `https://releases.alien.dev` |
| `ALIEN_AGENT_BINARY` | Path to a local `alien-agent` binary (skips download) | (auto-detect) |
| `ALIEN_DEPLOYMENT_INTERVAL` | Deployment loop interval (seconds) | `10` |
| `ALIEN_HEARTBEAT_INTERVAL` | Heartbeat interval (seconds) | `60` |

### Push Model Credentials

For push-model deployments, the manager needs cloud credentials:

- **AWS**: `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`
- **GCP**: `GOOGLE_APPLICATION_CREDENTIALS` or workload identity
- **Azure**: `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID`

---

## Documentation

See the [`docs/`](docs/) directory for detailed documentation:

- [Architecture](docs/00-foundations/00-architecture.md) — Crate structure and design decisions
- [Stack & Resources](docs/00-foundations/01-stack-and-resources.md) — Functions, containers, and resources
- [Build System](docs/00-foundations/03-build.md) — How builds work
- [Deployment Loop](docs/01-provisioning/01-deployment.md) — How deployments are provisioned
- [Manager Overview](docs/02-manager/00-overview.md) — Manager architecture
- [Manager API](docs/02-manager/06-api.md) — REST API reference
- [CLI Overview](docs/03-cli/00-overview.md) — CLI commands
- [Runtime](docs/04-runtime/00-runtime.md) — Function runtime
- [Platforms](docs/05-platforms/) — Platform-specific details
- [Testing](docs/09-testing/) — Testing framework

---

## License

See [LICENSE](LICENSE) for details.
