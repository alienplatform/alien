# Defining Your App

Your app is defined in `alien.ts`. This file describes what runs in the remote environment — it's a manifest, not a framework. Your app code stays the same.

## Containers

Bring an existing image, point to a Dockerfile, or build from source:

```typescript
import * as alien from "@alienplatform/core"

// Existing image
const app = new alien.Container("app")
  .image("myregistry.io/my-app:latest")
  .port(8080)
  .ingress("public")
  .build()

// Dockerfile
const worker = new alien.Container("worker")
  .image("./worker/Dockerfile")
  .resources({ cpu: "0.5", memory: "512Mi" })
  .build()
```

On cloud platforms (AWS, GCP, Azure), containers run on VMs managed by Alien's orchestration system. On Kubernetes, they run as native Pods. Locally, Docker containers.

## Functions

Serverless functions — deployed as Lambda (AWS), Cloud Run (GCP), Container Apps (Azure), or Docker containers (local/K8s):

```typescript
const api = new alien.Function("api")
  .code({ type: "source", src: "./api", toolchain: { type: "typescript" } })
  .ingress("public")
  .permissions("execution")
  .build()
```

### Ingress

- `"public"` — HTTPS endpoint with a `*.vpc.direct` domain and managed TLS, or your custom domain.
- `"private"` — no public URL. Reachable from other resources in the stack or via [commands](06-commands.md).

## Cloud Resources

Object storage, KV stores, queues — provisioned in the customer's cloud, abstracted across platforms:

```typescript
// S3 on AWS, Cloud Storage on GCP, Blob Storage on Azure, filesystem locally
const data = new alien.Storage("data").build()

// DynamoDB on AWS, Firestore on GCP, Table Storage on Azure, SQLite locally
const cache = new alien.Kv("cache").build()

// SQS on AWS, Pub/Sub on GCP, Service Bus on Azure, in-memory locally
const tasks = new alien.Queue("tasks").build()
```

Use these with the optional [cloud-agnostic bindings](05-bindings.md), or access the underlying cloud resources directly with any SDK.

## Putting It Together

```typescript
import * as alien from "@alienplatform/core"

const api = new alien.Function("api")
  .code({ type: "source", src: "./api", toolchain: { type: "typescript" } })
  .ingress("public")
  .permissions("execution")
  .build()

const data = new alien.Storage("data").build()
const cache = new alien.Kv("cache").build()

export default new alien.Stack("my-saas")
  .add(api, "live")
  .add(data, "frozen")
  .add(cache, "frozen")
  .permissions({
    profiles: {
      execution: {
        data: ["storage/data-read", "storage/data-write"],
        cache: ["kv/data-read", "kv/data-write"],
      },
    },
  })
  .build()
```

### Frozen vs Live

Alien splits deployment into two phases:

**Initial setup** — runs once, with elevated permissions (often admin-level). Creates IAM roles, VPCs, storage buckets. This is what the customer's admin runs during onboarding.

**Ongoing operations** — runs on every release, with least-privilege permissions. Deploys new code, updates functions and containers. Can run remotely (push) or locally (pull) — a different process, on a different machine, with different permissions.

That's why frozen/live matters:

- `"frozen"` — deployed during initial setup. Storage, KV, queues. After setup, Alien doesn't need permissions to touch them.
- `"live"` — deployed during ongoing operations. Functions, containers. Alien only needs the minimum permissions to update these.

### Permission Profiles

Resources in the stack are only accessible if explicitly granted. A function can't read from storage unless you say so. This follows the principle of least privilege — important when deploying to environments you don't control.

## Environment Variables

```typescript
const api = new alien.Function("api")
  .code({ type: "source", src: "./api", toolchain: { type: "typescript" } })
  .environment({
    LOG_LEVEL: "info",
    FEATURE_FLAG: "true",
  })
  .build()
```

Environment variables can also be set per-deployment when onboarding customers.

## Next

- [Developing](03-developing.md) — `alien dev` on all platforms
