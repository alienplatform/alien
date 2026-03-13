# Compute Resources

Alien provides three resource types for running code: **Function**, **Container**, and **Worker**. This doc explains how they differ and when to use each.

## Quick Comparison

|  | Function | Container | Worker |
|---|---|---|---|
| **Execution** | Event-driven, ephemeral | Always-on, replicated | Always-on, single instance |
| **Works without Docker** | ✅ | ❌ | ✅ |
| **Platforms** | AWS, GCP, Azure, Local | AWS, GCP, Azure, Local (Docker) | Local only |
| **Scale to zero** | ✅ | ❌ | ❌ |
| **Internal networking** | ❌ | ✅ DNS (`.svc`) | ❌ |
| **Can run existing images** | ❌ | ✅ | ❌ |
| **Triggers** | Storage, Queue, Cron | ❌ | ❌ |
| **Public access** | `.ingress("public")` | `.expose({ port: 80 })` | Port available on machine |
| **Port allocation** | Dynamic (`--port`) | Static (`.port(3000)`) | Dynamic (`--port`) |
| **Service-to-service** | Bindings (gRPC) | DNS (`.svc`) | Bindings (gRPC) |

## Function

Event-driven code that runs in response to triggers: HTTP requests, storage uploads, queue messages, cron schedules.

### Platform Mapping

| Platform | Implementation |
|----------|----------------|
| AWS | Lambda |
| GCP | Cloud Run |
| Azure | Container Apps |
| Local | Native process |

### Characteristics

- **Scales to zero**: No cost when idle
- **Dynamic ports**: alien-runtime passes `--port` to the process
- **Stateless**: No persistent local storage
- **Bindings-based communication**: Uses gRPC bindings to call other resources

### When to Use

Use Function for:
- REST/GraphQL APIs with intermittent traffic
- Event processors (storage uploads, queue messages)
- Scheduled tasks (cron jobs)
- Webhook handlers
- Portable agents that need to work across cloud and on-premise

Function is the only resource that works everywhere—including on-premise without Docker.

### Example

```typescript
const storage = new alien.Storage("uploads").build()
const queue = new alien.Queue("jobs").build()

// HTTP API (public access)
const api = new alien.Function("api")
  .code({ type: "source", src: "./api" })
  .ingress("public")
  .link(storage)
  .link(queue)
  .build()

// Process uploads
const processor = new alien.Function("processor")
  .trigger({ type: "storage", storage, events: ["created"] })
  .code({ type: "source", src: "./processor" })
  .link(storage)
  .build()

// Daily cleanup
const cleanup = new alien.Function("cleanup")
  .trigger({ type: "schedule", cron: "0 2 * * *" })
  .code({ type: "source", src: "./cleanup" })
  .link(storage)
  .build()
```

### Networking

Functions use **bindings** for service-to-service communication:

```typescript
const ctx = await AlienContext.fromEnv();

// Call another Function
const otherFn = await ctx.getBindings().loadFunction('other-function');
await otherFn.invoke({ method: 'POST', path: '/process', body: {...} });

// Access Storage/Queue/KV
const storage = await ctx.getBindings().loadStorage('data');
await storage.put('key', data);
```

No DNS, no internal networking—each Function instance is independent.

## Container

Always-on containerized workloads with internal networking.

### Platform Mapping

| Platform | Implementation |
|----------|----------------|
| AWS | EC2 + containerd + Horizon |
| GCP | GCE + containerd + Horizon |
| Azure | Azure VMs + containerd + Horizon |
| Local | Docker |

### Characteristics

- **Always running**: Minimum replicas always up (never scales to zero)
- **Replica-based scaling**: Auto-scales between min and max based on CPU/memory
- **Static ports**: Application binds to a fixed port via `.port(3000)`
- **Internal DNS**: Containers discover each other via `.svc` domains
- **Can run existing images**: postgres:16, redis:alpine, nginx

### Horizon Integration

Containers use Horizon for orchestration on cloud platforms. Alien handles infrastructure (VMs, load balancers, volumes); Horizon handles scheduling and networking.

```
┌─────────────────────────────────┐
│  Customer Cloud Account         │
│  ┌─────────┐  ┌─────────┐       │
│  │horizond │  │horizond │       │
│  └────┬────┘  └────┬────┘       │
│       └──────┬─────┘            │
│              │ HTTPS (outbound) │
└──────────────┼──────────────────┘
               ▼
      Horizon Control Plane
```

### When to Use

Use Container for:
- Databases: PostgreSQL, Redis, MongoDB
- Microservices with internal networking
- WebSocket servers (persistent connections)
- GPU workloads
- High-throughput APIs (cheaper at scale than serverless)

Containers require Docker on Local platform. They don't work on-premise without container infrastructure.

### Example

```typescript
// Database (existing image)
const db = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .cpu(4).memory("16Gi")
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")
  .port(5432)
  .build()

// Cache (existing image)
const redis = new alien.Container("redis")
  .code({ type: "image", image: "redis:alpine" })
  .cpu(2).memory("4Gi")
  .replicas(1)
  .port(6379)
  .build()

// API (built from source)
const api = new alien.Container("api")
  .code({ type: "source", toolchain: { type: "node" }, src: "./api" })
  .cpu(1).memory("2Gi")
  .minReplicas(2).maxReplicas(20)
  .port(3000)
  .expose({ port: 80 })
  .link(db)      // DNS: postgres.svc
  .link(redis)   // DNS: redis.svc
  .build()

export default new alien.Stack("cloud-app")
  .add(db, "live")
  .add(redis, "live")
  .add(api, "live")
  .build()
```

### Networking

Containers use **DNS** for service discovery:

```typescript
const { Client } = require('pg');
const client = new Client({
  host: 'postgres.svc',
  port: 5432
});
```

Stateful services get per-replica DNS:
- `postgres.svc` → all replicas
- `postgres-0.postgres.svc` → specific replica

See [Containers Overview](../07-containers/1-overview.md) for details.

## Worker

Always-on native process for endpoints. Currently Local-only.

### Platform Mapping

| Platform | Implementation |
|----------|----------------|
| AWS | Not supported |
| GCP | Not supported |
| Azure | Not supported |
| Local | Native process |

### Characteristics

- **Single instance**: No scaling (always exactly one process)
- **Dynamic ports**: alien-runtime passes `--port` to the process
- **Bindings-based communication**: Same as Function

### When to Use

Use Worker for:
- Endpoint agents on employee laptops
- Desktop tools distributed via Jamf or Intune
- On-premise daemons without Docker

Worker is essentially "Function without event triggers, Local-only."

### Example

```typescript
const agent = new alien.Worker("agent")
  .code({ type: "source", toolchain: { type: "node" }, src: "./agent" })
  .environment({
    API_URL: "https://api.yourcompany.com"
  })
  .build()

export default new alien.Stack("endpoint-agent")
  .add(agent, "live")
  .build()
```

### Networking

Workers use **bindings** (like Function):

```typescript
const ctx = await AlienContext.fromEnv();
const api = await ctx.getBindings().loadFunction('api');
await api.invoke({ method: 'POST', path: '/report', body: {...} });
```

Or direct HTTP to external services:

```typescript
await fetch(process.env.API_URL + '/report', {
  method: 'POST',
  body: JSON.stringify({...})
});
```

## API Reference

### `.trigger()` — Function Only

Only Function supports triggers:

```typescript
// Storage trigger
new alien.Function("processor")
  .trigger({ type: "storage", storage, events: ["created"] })

// Queue trigger
new alien.Function("worker")
  .trigger({ type: "queue", queue })

// Schedule trigger
new alien.Function("cleanup")
  .trigger({ type: "schedule", cron: "0 2 * * *" })
```

### `.port()` — Container Only

Only Container supports `.port()` because only Container has internal networking:

```typescript
new alien.Container("api")
  .port(3000)  // Internal networking via DNS
```

### `.expose()` — Container Only

Only Container supports `.expose()` for public access via load balancer:

```typescript
new alien.Container("api")
  .port(3000)
  .expose({ port: 80 })
```

For Function, use `.ingress("public")` instead.

## Remote Commands

Remote commands let the control plane send commands to deployments without inbound networking. They require alien-runtime:

| Resource | Command Support |
|----------|-----------------|
| Function | Always |
| Container (source) | Yes |
| Container (image) | No |
| Worker | Always |

Resources built from source include alien-runtime and support remote commands. Existing images (postgres:16, redis:alpine) don't have alien-runtime and don't support commands.

## Stack Patterns

### Pattern 1: Maximum Portability (Functions Only)

Works in customer cloud OR on-premise without Docker:

```typescript
const storage = new alien.Storage("data").build()
const queue = new alien.Queue("jobs").build()

const api = new alien.Function("api")
  .code({ type: "source", src: "./api" })
  .ingress("public")
  .link(storage)
  .link(queue)
  .build()

const processor = new alien.Function("processor")
  .trigger({ type: "queue", queue })
  .code({ type: "source", src: "./processor" })
  .link(storage)
  .build()

export default new alien.Stack("portable-app")
  .add(storage, "frozen")
  .add(queue, "frozen")
  .add(api, "live")
  .add(processor, "live")
  .build()
```

Deploy anywhere:

```bash
alien deploy customer-aws --platform aws     # Lambda + S3 + SQS
alien deploy customer-gcp --platform gcp     # Cloud Run + GCS + Pub/Sub
alien deploy customer-onprem --platform local # Native + directories
```

### Pattern 2: Containers with Networking

For customer cloud with internal networking:

```typescript
const db = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")
  .port(5432)
  .build()

const api = new alien.Container("api")
  .code({ type: "source", src: "./api" })
  .minReplicas(2).maxReplicas(20)
  .port(3000)
  .expose({ port: 80 })
  .link(db)  // DNS: postgres.svc
  .build()

export default new alien.Stack("cloud-app")
  .add(db, "live")
  .add(api, "live")
  .build()
```

Cannot deploy without Docker (Stack contains Containers).

### Pattern 3: Hybrid (Functions + Containers)

Serverless API with always-on database:

```typescript
const storage = new alien.Storage("data").build()

const db = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")
  .port(5432)
  .build()

const api = new alien.Function("api")
  .code({ type: "source", src: "./api" })
  .ingress("public")
  .link(db)
  .link(storage)
  .build()

export default new alien.Stack("hybrid-app")
  .add(storage, "frozen")
  .add(db, "live")
  .add(api, "live")
  .build()
```

API scales to zero, database stays running.

### Pattern 4: Endpoint Agents (Workers)

For employee laptops:

```typescript
const agent = new alien.Worker("agent")
  .code({ type: "source", src: "./agent" })
  .environment({
    API_URL: "https://api.yourcompany.com"
  })
  .build()

export default new alien.Stack("endpoint-agent")
  .add(agent, "live")
  .build()
```

## Common Use Cases

| Use Case | Resource | Rationale |
|----------|----------|-----------|
| Agent (cloud + on-prem) | Function | Works everywhere without Docker |
| REST API (low traffic) | Function | Scales to zero, pay per request |
| REST API (high traffic) | Container | Always-on, cheaper at scale |
| Database | Container | Persistent storage + DNS |
| Microservices | Container | Internal networking via DNS |
| Event processor | Function | Triggered by storage/queue |
| Scheduled job | Function | Cron trigger |
| Endpoint agent | Worker | Single process per laptop |
| WebSocket server | Container | Persistent connections |

## Decision Flow

1. **Must work without Docker?** → Function (cloud + local) or Worker (local only)
2. **Event-driven?** (HTTP, storage, queue, cron) → Function
3. **Needs internal networking?** (services calling via DNS) → Container
4. **Endpoint daemon, local only?** → Worker
5. **Running existing images?** (postgres, redis) → Container
