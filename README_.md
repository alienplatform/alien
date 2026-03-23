# Alien: Ship to Your Customer's Cloud

Every enterprise sales call ends the same way: *"Our data is sensitive. Can you deploy this into our cloud?"*

The usual approach: ship a Docker image or Helm chart. Let the customer run it. But then you lose control—no auto-updates, no logs, no way to debug without a painful support call. Every customer runs a different version. Ensuring AI model output quality is impossible. You're back to the 90s.

Alien provides infrastructure for deploying software to your customers' cloud, while keeping it fully managed by you.

Deploy to **AWS**, **GCP**, **Azure**, **Kubernetes**, or a **single VM**. Lightweight deployments or full applications — they run entirely in your customer's environment.

---

## Getting Started

### Local Development

```bash
cd my-project
alien dev
```

Starts alien-server locally with SQLite, builds your stack, and deploys everything with Docker. No cloud credentials needed.

### Self-Hosted

Run your own alien-server. Data is stored in SQLite — no external database required.

```bash
docker run -d \
  -p 8080:8080 \
  -e AWS_ACCESS_KEY_ID=$AWS_ACCESS_KEY_ID \
  -e AWS_SECRET_ACCESS_KEY=$AWS_SECRET_ACCESS_KEY \
  -e AWS_REGION=us-east-1 \
  -e OTLP_ENDPOINT=http://your-grafana:4318 \
  -v alien-data:/data \
  ghcr.io/alienplatform/alien-server:latest
```

On first run, alien-server prints an admin API key:

```
Admin API key: ax_admin_abc123def456...
Save this key — it won't be shown again.
```

### Build, Release, Deploy

```bash
# Build OCI images locally
alien build --platform aws

# Push images to your registry, create a release
alien release --server http://localhost:8080

# Create a deployment in AWS
alien deploy \
  --server http://localhost:8080 \
  --platform aws \
  --name production

# Check status
alien deployments ls --server http://localhost:8080
```

```
NAME        STATUS    PLATFORM  RELEASE
production  running   aws       rel_abc123
```

alien-server impersonates a service account in the target AWS account and provisions Lambda functions, IAM roles, S3 buckets, etc. via the deployment loop.

### Pull Model (Kubernetes)

For environments where alien-server can't reach cloud APIs directly:

```bash
helm install alien-agent alien/agent \
  --set syncUrl=https://alien.example.com \
  --set token=ax_dg_... \
  --set platform=kubernetes
```

The agent polls alien-server for updates and deploys using in-cluster credentials.

---

## Developer Workflow

### Build Your App

Define your stack in `alien.ts`. Use any framework — **Next.js**, **Hono**, **FastAPI**, **Axum**. Bring existing containers or use cloud-agnostic bindings:

```typescript
import { ai } from "@alienplatform/sdk"
import postgres from "postgres"

const db = postgres(process.env.DATABASE_URL)

export default async function Dashboard() {
  const sales = await db`SELECT * FROM sales`
  const customers = await fetch(process.env.CRM_API + "/customers")

  // Bedrock on AWS, Vertex AI on GCP, local model locally
  const summary = await ai().generate(`Summarize: ${sales}`)

  return <SalesInsights data={sales} summary={summary} />
}
```

### One API, Every Platform

The SDK abstracts away cloud differences:

| Abstraction | AWS | GCP | Azure | Local |
|-------------|-----|-----|-------|-------|
| `storage()` | S3 | Cloud Storage | Blob Storage | Filesystem |
| `ai()` | Bedrock | Vertex AI | Azure OpenAI | Local model |
| `queue()` | SQS | Pub/Sub | Service Bus | In-memory |
| `kv()` | DynamoDB | Firestore | Table Storage | SQLite |

The SDK is entirely optional. Call low-level cloud APIs directly or use any existing libraries.

### Remote Commands

Invoke code on deployments from your control plane. Zero inbound networking. Zero open ports:

```typescript
import { storage, command } from "@alienplatform/sdk"

command("generate-report", async ({ startDate, endDate }) => {
  const events = await storage("data").list({ prefix: "events/" })
  return { report: await aggregate(events) }
})
```

```bash
alien command invoke \
  --server http://localhost:8080 \
  --deployment production \
  --command generate-report \
  --params '{"startDate": "2025-01-01", "endDate": "2025-12-31"}'
```

---

## alien-server

alien-server is the control plane. It stores releases, manages deployments to remote environments, dispatches commands, and collects telemetry.

```
┌────────────────────────────────────┐
│          alien-server              │
│                                    │
│  ┌─────────┐  ┌────────────────┐  │
│  │ REST API │  │ Deployment Loop│  │
│  └────┬────┘  └───────┬────────┘  │
│       │               │           │
│  ┌────┴─────────┐  ┌──┴─────────┐│
│  │Command Server│  │ Telemetry  ││
│  └──────────────┘  └────────────┘│
│       │                          │
│  ┌────┴──────────────────────┐   │
│  │       SQLite (Turso)      │   │
│  └───────────────────────────┘   │
└────────────────────────────────────┘
       ▲               │
       │               ▼
    CLI / SDK     Cloud APIs
   (push releases,  (provision resources
    create deploys)  in remote environments)
```

**Two deployment models:**

- **Push.** alien-server impersonates a service account in the remote environment and calls cloud APIs directly. For AWS, GCP, Azure.
- **Pull.** An Operator in the remote environment polls alien-server and deploys locally. For Kubernetes, edge devices, airgapped environments.

**Embeddable.** alien-server is a library with a builder API. Swap providers for storage, credentials, telemetry, and authentication to embed it in your own hosting service.

See [Server Documentation](docs/02-server/) for architecture, configuration, and the full API surface.

---

## Alien Containers

If you have 1,000 customers and your app runs on Kubernetes, you have to manage 1,000 remote K8s clusters. That's an operational nightmare.

**Alien Containers** is Kubernetes built from first principles for BYOC:

- **Control plane** in *your* cloud — one instance, serverless, scales to zero
- **Data plane** in *their* cloud — stateless VMs that can terminate anytime

Instead of 1,000 orchestration systems, you manage one. Container autoscaling, machine autoscaling, GPU capacity groups, local NVMe, stateful containers, HTTP-based autoscaling, built-in observability via eBPF.

---

## On-Prem & Airgapped

For highly regulated customers — finance, defense, government — who can't allow external network access.

**On-prem:** Deploy to private Kubernetes clusters using the pull model. Connect Alien resources to the customer's existing infrastructure — their Kafka, their MinIO, their Postgres.

**Airgapped:** The CLI bridges disconnected networks. Sync releases and telemetry to a local folder, then transfer between networks however works — toggle VPN, carry files, or use a secure file transfer system.

---

## Documentation

- [Server Quickstart](docs/02-server/09-quickstart.md) — Deploy your first app with alien-server
- [Server Architecture](docs/02-server/00-overview.md) — How alien-server works internally
- [Entities](docs/02-server/01-entities.md) — Data model: deployments, releases, tokens
- [API Reference](docs/02-server/02-api.md) — Complete endpoint documentation
- [Deployment Loop](docs/02-server/03-deployment-loop.md) — How deployments are provisioned in remote environments
- [State Sync](docs/02-server/04-state-sync.md) — Push vs pull protocol
- [Telemetry](docs/02-server/05-telemetry.md) — OTLP ingestion and forwarding (logs, traces, metrics)
- [Releases & Images](docs/02-server/06-artifact-registry.md) — How container images get to platform registries
- [Commands](docs/02-server/07-commands.md) — Remote command protocol
- [Configuration](docs/02-server/08-configuration.md) — Environment variables reference
- [Authentication](docs/02-server/10-auth.md) — Token security and scope enforcement
- [Local Development](docs/02-server/11-local-development.md) — How `alien dev` works
- [Architecture](docs/00-foundations/00-architecture.md) — Crate structure and design decisions
