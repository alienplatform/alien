# Containers - Overview

Containers run long-running workloads on compute instances (EC2, GCE, Azure VMs) in remote environments.

Unlike Functions (ephemeral, event-driven), Containers are **always-on**. Web servers, APIs, databases, ML inference servers, background workers.

## Use Cases

**Web Services & APIs:**
- REST/GraphQL APIs with persistent connections
- WebSocket servers
- Server-sent events (SSE)

**Databases & Stateful Systems:**
- PostgreSQL, MySQL, Redis
- ClickHouse, Elasticsearch
- Distributed databases with replication (CockroachDB, Cassandra)

**AI/ML Workloads:**
- Training jobs
- Inference servers with GPU
- Model serving with local cache

**Zero-Disk Architectures:**
- Search platforms (Quickwit, Turbopuffer) with S3 backing + local NVMe cache
- Analytics engines with object storage backend

## Key Innovation: Alien + Horizon

Alien Containers use **Horizon** for container orchestration - a lightweight alternative to Kubernetes designed for small-scale deployments (≤10 machines per cluster).

```
┌─────────────────────────────┐
│  Remote Environment         │
│                             │
│  ┌─────────┐  ┌─────────┐  │
│  │horizond │  │horizond │  │
│  └────┬────┘  └────┬────┘  │
│       │            │        │
│       └────────────┘        │
│            │                │
│            │ HTTPS          │
│            │ (outbound)     │
└────────────┼────────────────┘
             │
             ▼
    ┌────────────────────────┐
    │ Horizon Control Plane  │
    │ (Cloudflare Durable    │
    │  Objects per cluster)  │
    └────────────────────────┘
```

**Division of responsibility:**

| Component | Responsibility |
|-----------|---------------|
| **Alien** | Infrastructure (VMs, load balancers, volumes), instance selection, machine autoscaling |
| **Horizon** | Container orchestration (scheduling, replica autoscaling), networking, service discovery |

## How It Works

**1. Developer declares containers:**
```typescript
const api = new alien.Container("api")
  .cpu(1).memory("2Gi")
  .minReplicas(2).maxReplicas(20)
  .port(3000)
  .expose("http")
```

**2. Platform prepares Horizon cluster (before deployment):**
- Creates cluster in Horizon (if not exists) using Platform JWT
- Receives management token and machine token (once only)
- Encrypts and stores tokens in platform database (`deployment.horizonConfig`)
- Adds machine token as built-in environment variable (synced to vault)

**3. Alien provisions infrastructure:**
- Determines required compute (e.g., m7g.2xlarge on AWS)
- Creates secrets vault (if not exists - for machine token storage)
- Syncs machine token to vault (Parameter Store/Secret Manager/Key Vault)
- Creates Auto Scaling Group with user data that fetches token from vault
- Provisions volumes (for stateful containers with `.persistentStorage()`)
- Creates load balancer (for containers with `.expose()`)

**4. Alien creates containers in Horizon:**
- Calls Horizon API with container definitions (management token auth)
- Includes pre-provisioned volume details (VolumeTarget enums)
- Includes load balancer target details (LoadBalancerTarget enum)

**5. Machines boot and horizond registers:**
- User data fetches machine token from vault (using IAM role/service account)
- Starts `horizond` with token in memory
- Polls Horizon: "I have 8 vCPU, 32 GB, in us-east-1a" (machine token auth)

**6. Horizon scheduler assigns containers:**
- "Machine i-abc123: run api replica 1"
- "Machine i-def456: run api replica 2"

**7. horizond starts containers:**
- Pulls image from Alien's artifact registry
- Attaches volumes (for stateful containers - calls cloud APIs)
- Starts container via containerd
- Creates TCP proxy + registers with load balancer (for exposed containers)
- Reports metrics back to Horizon

**8. Horizon handles replica autoscaling:**
- Monitors CPU/memory metrics
- Scales replicas up/down based on load (stateless containers only)
- Stateful containers use fixed replica counts

**9. Alien handles machine autoscaling:**
- Polls Horizon's capacity plan API (management token auth)
- Scales each ASG based on Horizon's recommendations per capacity group
- No threshold logic - just applies what Horizon calculates

**Key abstractions:** 
- You write `.persistentStorage("500Gi")` in Alien (user-facing API)
- Alien provisions the volume and passes `VolumeTarget` enum to Horizon (implementation detail)
- horizond attaches the volume at runtime using cloud-specific APIs

**Security abstraction:**
- Platform manages Horizon authentication (JWT + tokens)
- Machine tokens stored in cloud vault (Parameter Store/Secret Manager/Key Vault)
- VMs fetch tokens securely at boot (never exposed in user data)
- Zero configuration required from developer

## Containers vs Functions

See [Compute Resources](../00-compute-resources.md) for a detailed comparison.

**Use Containers for:**
- Long-running processes
- Persistent connections (WebSockets, SSE)
- Stateful workloads (databases)
- High throughput (cheaper at scale)
- GPU workloads
- Running existing images (postgres, redis)

**Use Functions for:**
- Event-driven workloads
- Intermittent traffic (scale to zero)
- Stateless operations
- Maximum portability (works without Docker)

## Integration with Existing Alien Patterns

Containers reuse all existing Alien infrastructure:

**Same Deployment System:**
- Uses `alien-deployment` crate (Pending → InitialSetup → Provisioning → Running)
- Same state machine, same resumability
- Frozen resources (ComputePools) + Live resources (Containers)

**Same Resource Model:**
- Defined in `alien.ts` alongside Functions, Storage, Queue
- Uses `.link()` to connect to other resources
- Same permission profiles system

**Same Build System:**
- Uses `alien build` to compile stack and build images
- Pushes to Alien's ArtifactRegistry
- Same toolchain support (Node, Python, Rust)

**Example Stack:**
```typescript
const storage = new alien.Storage("data").build()
const queue = new alien.Queue("jobs").build()

const api = new alien.Container("api")
  .code({ type: "source", toolchain: { type: "node" }, src: "./api" })
  .minReplicas(2).maxReplicas(10)
  .link(storage)
  .link(queue)
  .build()

const worker = new alien.Container("worker")
  .code({ type: "source", toolchain: { type: "node" }, src: "./worker" })
  .replicas(3)
  .link(queue)
  .link(storage)
  .build()

export default new alien.Stack("my-app")
  .add(storage, "frozen")
  .add(queue, "frozen")
  .add(api, "live")
  .add(worker, "live")
  .build()
```

## Next

- **[Quickstart](2-quickstart.md)** — Deploy your first container
- **[Architecture](2-architecture.md)** — How Alien and Horizon work together
- **[Resource API](3-resource-api.md)** — Container configuration reference
- **[Infrastructure](4-infrastructure.md)** — ComputePools and instance selection
- **[Deployment Flow](5-deployment-flow.md)** — Step-by-step deployment process
- **[Machine Autoscaling](6-machine-autoscaling.md)** — How Alien scales infrastructure

