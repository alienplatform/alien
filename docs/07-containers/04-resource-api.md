# Containers - Resource API Reference

Complete reference for the `Container` resource type.

## Basic Structure

```typescript
import * as alien from "@alienplatform/core"

const container = new alien.Container(name: string)
  .code(config: CodeConfig)
  .cpu(vcpus: number)
  .memory(size: string)
  // ... configuration methods
  .build()
```

## Code Source

### `.code(config: CodeConfig)`

Specifies the container code source.

**Pre-built image (registry or local):**
```typescript
.code({
  type: "image",
  image: string  // Registry image or local Docker image
})
```

All images are pulled and re-pushed to the developer's registry for reproducibility.

**Source code:**
```typescript
.code({
  type: "source",
  src: string,                // Path to source directory
  toolchain: ToolchainConfig  // Rust, TypeScript, or Docker
})
```

**Examples:**

```typescript
// TypeScript application
.code({
  type: "source",
  src: "./api",
  toolchain: { type: "typescript" }
})

// Rust application
.code({
  type: "source",
  src: "./worker",
  toolchain: { 
    type: "rust",
    binaryName: "worker"
  }
})

// Dockerfile build
.code({
  type: "source",
  src: "./gateway",
  toolchain: { type: "docker" }
})

// Dockerfile with build args
.code({
  type: "source",
  src: "./gateway",
  toolchain: { 
    type: "docker",
    dockerfile: "Dockerfile.prod",
    buildArgs: { VERSION: "1.0.0" }
  }
})

// Registry image (pulled and re-pushed)
.code({
  type: "image",
  image: "postgres:16"
})

// Local Docker image (exported and pushed)
.code({
  type: "image",
  image: "my-custom-gateway:v1"
})
```

## Compute Resources

### `.cpu(vcpus: number)`

Number of vCPUs per replica.

**Default:** `1`

```typescript
.cpu(1)     // 1 vCPU
.cpu(2)     // 2 vCPUs
.cpu(0.5)   // 500 millicores (0.5 vCPU)
.cpu(4)     // 4 vCPUs
```

### `.memory(size: string)`

Memory limit per replica.

**Default:** `"2Gi"`

**Format:** `"<number>Mi"` or `"<number>Gi"`

```typescript
.memory("512Mi")   // 512 MB
.memory("2Gi")     // 2 GB
.memory("16Gi")    // 16 GB
.memory("128Gi")   // 128 GB
```

### `.gpu(config: GpuConfig)`

Request GPU resources. Optional.

```typescript
.gpu({
  type: "nvidia-t4" | "nvidia-a100" | "nvidia-h100",
  count: number
})
```

**Examples:**
```typescript
// Single T4 GPU for inference
.gpu({ type: "nvidia-t4", count: 1 })

// 8x A100 GPUs for training
.gpu({ type: "nvidia-a100", count: 8 })
```

**Note:** GPU services automatically get assigned to GPU compute pools.

## Storage

### `.ephemeralStorage(size: string)`

Fast local disk per replica. Data is **lost on container restart**.

**Default:** `"10Gi"` (container overlay filesystem)

**Use cases:**
- Build caches (npm, pip, cargo)
- Temporary files during processing
- Zero-disk databases with object storage backing (Quickwit, Turbopuffer)
- Local query/index cache

```typescript
.ephemeralStorage("10Gi")    // Default (container overlay)
.ephemeralStorage("100Gi")   // Large cache
.ephemeralStorage("500Gi")   // Zero-disk database cache (triggers storage-optimized instance with NVMe)
.ephemeralStorage("1Ti")     // Very large cache
```

**Performance:** 
- ≤10Gi: Container overlay filesystem (~10K IOPS)
- >200Gi: Triggers storage-optimized instances with NVMe SSD (~400K IOPS, 4GB/s throughput)

### `.persistentStorage(size: string)`

Durable block disk per replica. Data **survives restarts**.

**Requires:** `.stateful(true)` and `.replicas(count)` (fixed count)

```typescript
.persistentStorage("100Gi")   // 100 GB persistent disk
.persistentStorage("500Gi")   // 500 GB (common for databases)
.persistentStorage("1Ti")     // 1 TB
```

**What happens behind the scenes:**
1. During deployment, Alien provisions block volumes (EBS on AWS, Persistent Disks on GCP, Managed Disks on Azure)
2. One volume per replica, distributed across availability zones
3. Alien passes volume details to Horizon as `VolumeTarget` enums
4. horizond attaches volumes at runtime when starting containers

**Behavior when set:**
- Requires `stateful: true` with fixed `replicas` (cannot use `minReplicas`/`maxReplicas`)
- One block disk per replica
- Volumes survive container restarts and machine failures
- Replicas are zone-pinned to their volume's availability zone
- Ordered rollouts (update one replica at a time)

See [Storage](5-storage.md) and [Stateful Containers](9-stateful-services.md) for detailed semantics.

## Scaling

### Option A: Auto-Scaled (Stateless Containers)

For containers that can scale horizontally based on load.

```typescript
.minReplicas(count: number)      // Default: 1
.maxReplicas(count: number)      // Default: 10
.autoScale(config: AutoScaleConfig)
```

**Auto-scale configuration:**
```typescript
.autoScale({
  targetCpuPercent?: number,      // Default: 70
  targetMemoryPercent?: number,   // Default: 80
})
```

**Examples:**
```typescript
// Scale from 2 to 20 based on CPU
new alien.Container("api")
  .minReplicas(2)
  .maxReplicas(20)
  .autoScale({ targetCpuPercent: 70 })

// Scale based on memory pressure
new alien.Container("cache")
  .minReplicas(3)
  .maxReplicas(15)
  .autoScale({ targetMemoryPercent: 80 })
```

**How it works:**
- Scheduler checks metrics every 5 seconds
- If avg CPU > target: scale up
- If avg CPU < target: scale down
- Bounded by min/max replicas

### Option B: Fixed Replicas (Stateful Containers)

For stateful workloads where Replica count is fixed.

```typescript
.replicas(count: number)
.stateful(enabled: boolean)
```

**Examples:**
```typescript
// Single-instance database
new alien.Container("postgres")
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")

// 3-node quorum (CockroachDB, etc.)
new alien.Container("cockroach")
  .replicas(3)
  .stateful(true)
  .persistentStorage("1Ti")
```

**Validation:** Cannot use both `replicas` and `minReplicas/maxReplicas`.

## Ports and Networking

### `.port(port: number)`

Adds an internal-only port. **Automatically creates DNS records** for service discovery.

```typescript
.port(3000)   // HTTP (DNS: {service-id}.svc)
.port(5432)   // PostgreSQL (DNS: postgres.svc)
.port(8080)   // Common alt HTTP
```

**DNS behavior:**
- **Automatic for all containers** - no opt-in required
- Stateless containers: `{service-id}.svc` returns all healthy replica IPs
- Stateful containers: Both `{service-id}.svc` (all replicas) and `{service-id}-{ordinal}.{service-id}.svc` (individual replicas)
- TTL: 5 seconds (updates quickly when replicas scale)
- Client-side load balancing (DNS returns multiple IPs)

### `.ports(ports: number[])`

Adds multiple internal-only ports. All accessible via DNS.

```typescript
.ports([8080, 9090])              // HTTP + metrics
.ports([26257, 8080])             // CockroachDB SQL + admin UI
.ports([5432, 6543])              // PostgreSQL + pgbouncer
```

### `.expose(protocol: "http" | "tcp")`

Exposes the first/primary port publicly via load balancer.

```typescript
.port(3000).expose("http")   // HTTPS on 443 with TLS certificate
.port(5432).expose("tcp")    // TCP passthrough on 5432, no TLS
```

**Protocols:**
- `"http"` → HTTPS load balancer on port 443 with TLS certificate (ALB, Cloud Load Balancer, App Gateway)
- `"tcp"` → TCP load balancer on the actual port, no TLS (NLB, TCP Load Balancer, Azure LB)

**Note:** Currently only one port can be exposed publicly per container. Use `.exposePort()` to specify which port.

### `.exposePort(port: number, protocol: "http" | "tcp")`

Exposes a specific port publicly (for multi-port containers).

```typescript
// Multi-port API with metrics
new alien.Container("api")
  .port(8080).port(9090)
  .exposePort(8080, "http")  // Expose API on HTTPS
  // 9090 stays internal for metrics scraping
  .build()

// Internal API (all ports private)
new alien.Container("internal-api")
  .ports([8080, 9090, 9091])
  // No .expose() or .exposePort() = all internal
  .build()
```

**What gets created:**
- **DNS (automatic)**: All ports accessible internally via `{service-id}.svc`
- **Load balancer (opt-in)**: Only for exposed ports
  - `"http"` → HTTPS on 443 with auto-managed TLS certificate
  - `"tcp"` → TCP passthrough on the actual port number
- **TLS certificate**: Automatic for HTTP exposure

See [Internal Networking](6a-internal-networking.md), [Service Discovery](6b-service-discovery.md), and [Public Networking](6c-public-networking.md) for details.

## Environment Variables

### `.environment(vars: Record<string, string>)`

Set environment variables for all replicas.

```typescript
.environment({
  NODE_ENV: "production",
  LOG_LEVEL: "info",
  API_KEY: "secret-value"
})
```

**Built-in variables (automatically injected):**
- `PORT` - First port defined (if any)
- `ALIEN_ORDINAL` - Replica ordinal (stateful services only): "0", "1", "2", ...

**Linked resources:**
When you `.link()` other resources, their binding variables are automatically added:

```typescript
const storage = new alien.Storage("data").build()
const queue = new alien.Queue("jobs").build()

const worker = new alien.Container("worker")
  .link(storage)   // Adds ALIEN_STORAGE_DATA_*
  .link(queue)     // Adds ALIEN_QUEUE_JOBS_*
  .build()
```

## Resource Linking

### `.link(resource: Resource)`

Link to other Alien resources (Storage, Queue, KV, etc.).

```typescript
const storage = new alien.Storage("uploads").build()
const db = new alien.Postgres("main").build()

const api = new alien.Container("api")
  .link(storage)  // Get storage bucket binding
  .link(db)       // Get database connection info
  .build()
```

**Effect:**
- Adds environment variables with resource bindings
- Adds appropriate permissions to service account
- Creates dependency (storage/db deployed before api)

## Advanced Configuration

### `.stateful(enabled: boolean)`

Enable stateful semantics. Usually set implicitly by `.persistentStorage()`.

**When true:**
- Replicas get stable ordinals: `{name}-0`, `{name}-1`, etc.
- `ALIEN_ORDINAL` environment variable injected
- DNS names: `{name}-{ordinal}.{name}.svc`
- Ordered rollouts (update 0 → wait → update 1 → ...)
- Per-replica persistent disks (if `.persistentStorage()` set)

```typescript
.stateful(true)
.replicas(3)       // Required with stateful
```

### `.pool(poolName: string)`

Assign service to a specific ComputePool. Optional.

```typescript
const gpuPool = new alien.ComputePool("gpu-pool")
  .gpu({ type: "nvidia-a100" })
  .build()

const inference = new alien.Container("inference")
  .gpu({ type: "nvidia-a100", count: 1 })
  .pool(gpuPool)   // Explicit assignment
  .build()
```

**Default behavior:** Containers are automatically assigned to pools:
- GPU containers → GPU pool (one per GPU type)
- High ephemeral storage (>200Gi) → Storage pool
- Everything else → General pool

See [Infrastructure](4-infrastructure.md) for pool details.

## Lifecycle

Containers are typically marked as `"live"` resources (can be updated frequently):

```typescript
export default new alien.Stack("my-app")
  .add(service, "live")
  .build()
```

**Lifecycle options:**
- `"live"` - Can be updated by platform (default for services)
- `"frozen"` - Created once, rarely changed (use for ComputePools)
- `"live-on-setup"` - Updated during initial setup, then frozen

## Validation Rules

The system validates your service configuration:

**1. Cannot mix auto-scaling and fixed replicas:**
```typescript
// ❌ Error: Cannot use both
.minReplicas(2).maxReplicas(10)
.replicas(5)

// ✅ Choose one:
.minReplicas(2).maxReplicas(10)  // Auto-scale

// ✅ Or:
.replicas(5)                      // Fixed
```

**2. persistentStorage requires stateful and fixed replicas:**
```typescript
// ❌ Error: Must enable stateful with fixed replicas
.persistentStorage("500Gi")
.minReplicas(1).maxReplicas(10)

// ✅ Correct:
.stateful(true)
.replicas(1)
.persistentStorage("500Gi")
```

**3. Stateful requires fixed replicas (cannot use autoscaling):**
```typescript
// ❌ Error: Cannot auto-scale stateful
.stateful(true)
.minReplicas(1).maxReplicas(10)

// ✅ Correct:
.stateful(true)
.replicas(3)
```

**4. Resource limits:**
```typescript
// CPU must be positive
.cpu(0)      // ❌ Error
.cpu(0.25)   // ✅ OK

// Memory must be at least 128Mi
.memory("64Mi")   // ❌ Error
.memory("512Mi")  // ✅ OK
```

## Complete Examples

### Stateless Web API
```typescript
const api = new alien.Container("api")
  .code({ type: "source", src: "./api", toolchain: { type: "typescript" } })
  .cpu(1)
  .memory("2Gi")
  .minReplicas(2)
  .maxReplicas(20)
  .autoScale({ targetCpuPercent: 70 })
  .port(3000)
  .expose("http")
  .environment({
    NODE_ENV: "production",
    LOG_LEVEL: "info"
  })
  .permissions("execution")
  .build()
```

### Stateful Database
```typescript
const postgres = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .cpu(4)
  .memory("16Gi")
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")  // Creates { size: "500Gi", mountPath: "/data" }
  .port(5432)
  .environment({
    POSTGRES_PASSWORD: "secret",
    POSTGRES_DB: "myapp"
  })
  .permissions("database")
  .build()
```

### Nginx from Dockerfile
```typescript
const gateway = new alien.Container("gateway")
  .code({
    type: "source",
    src: "./gateway",
    toolchain: {
      type: "docker",
      dockerfile: "Dockerfile.prod",
      buildArgs: { NGINX_VERSION: "1.25" }
    }
  })
  .cpu(2)
  .memory("4Gi")
  .minReplicas(2)
  .maxReplicas(10)
  .ports([80, 443])
  .exposePort(443, "http")
  .permissions("gateway")
  .build()
```

### GPU Inference Server
```typescript
const inference = new alien.Container("inference")
  .code({ type: "source", src: "./ml", toolchain: { type: "rust", binaryName: "inference-server" } })
  .cpu(8)
  .memory("32Gi")
  .gpu({ gpuType: "nvidia-a100", count: 1 })
  .ephemeralStorage("200Gi")  // Model weights cache
  .minReplicas(1)
  .maxReplicas(5)
  .port(8000)
  .expose("http")
  .permissions("inference")
  .build()
```

### Multi-Port Container
```typescript
const api = new alien.Container("api")
  .code({ type: "source", src: "./api", toolchain: { type: "typescript" } })
  .cpu(2).memory("4Gi")
  .port(8080)    // Main API
  .port(9090)    // Metrics
  .port(9091)    // Health/debug
  .exposePort(8080, "http")  // Only API is public
  // 9090 and 9091 stay internal
  .minReplicas(2).maxReplicas(10)
  .permissions("execution")
  .build()
```

### Multi-Tier Application
```typescript
const storage = new alien.Storage("uploads").build()
const queue = new alien.Queue("jobs").build()

const web = new alien.Container("web")
  .code({ type: "source", src: "./web", toolchain: { type: "typescript" } })
  .cpu(1).memory("2Gi")
  .minReplicas(2).maxReplicas(10)
  .port(3000)
  .expose("http")
  .link(storage)
  .link(queue)
  .permissions("web")
  .build()

const worker = new alien.Container("worker")
  .code({ type: "source", src: "./worker", toolchain: { type: "typescript" } })
  .cpu(2).memory("4Gi")
  .replicas(3)  // Fixed worker count
  .port(8080)   // Internal only
  .link(storage)
  .link(queue)
  .permissions("worker")
  .build()

const db = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .cpu(4).memory("16Gi")
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")
  .port(5432)
  // No .expose() = Internal (private database)
  .permissions("database")
  .build()

export default new alien.Stack("multi-tier-app")
  .add(storage, "frozen")
  .add(queue, "frozen")
  .add(web, "live")
  .add(worker, "live")
  .add(db, "live")
  .build()
```

## Next Steps

- **[Infrastructure](4-infrastructure.md)** - How compute pools and instance selection work
- **[Storage](5-storage.md)** - Deep dive on ephemeral and persistent storage
- **[Internal Networking](6a-internal-networking.md)** - Container networking and WireGuard mesh
- **[Service Discovery](6b-service-discovery.md)** - DNS for service-to-service communication
- **[Public Networking](6c-public-networking.md)** - Load balancers and Internet exposure
- **[Deployment Flow](7-deployment-flow.md)** - Step-by-step deployment process

