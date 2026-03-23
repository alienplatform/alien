# Stacks and Resources

## The Basics

An `alien.ts` defines a **Stack**. A Stack is a list of **Resources** to deploy.

```typescript
new alien.Stack("my-stack")
  .add(storage, "frozen")
  .add(fn, "live")
```

That's it. Two resources: `storage` and `fn`. One is frozen, one is live. We'll explain the difference shortly.

## Resources

A resource is something that gets deployed: a function, a storage bucket, a queue, a database.

```typescript
const storage = new alien.Storage("data-storage").build()
const fn = new alien.Function("processor").build()
const queue = new alien.Queue("tasks").build()
```

Resources are **platform-agnostic**. Alien translates them to platform-specific implementations at deploy time.

> We say "platform-agnostic" rather than "cloud-agnostic" because Local and Kubernetes are platforms too.

Examples:

- `Function` → Lambda (AWS), Cloud Run (GCP), Container Apps (Azure), Deployment (K8s), process (Local)
- `Storage` → S3 (AWS), Cloud Storage (GCP), Blob Storage (Azure), MinIO (K8s), filesystem (Local)
- `Queue` → SQS (AWS), Pub/Sub (GCP), Storage Queue (Azure), Redis (K8s/Local)
- `Kv` → DynamoDB (AWS), Firestore (GCP), Table Storage (Azure), Redis (K8s), sled (Local)

The full list of resource types is in [16-resources/](16-resources/).

## Frozen vs Live

When you add a resource to a stack, you specify its lifecycle:

```typescript
.add(storage, "frozen")
.add(fn, "live")
```

🧊 **Frozen**: Created once during initial setup. Rarely changes. Example: an S3 bucket for storing data.

🔁 **Live**: Updated on every deploy. Example: a Lambda function with your code.

### Why does this matter?

Permissions.

Alien manages stacks remotely. It needs permissions to update resources. But frozen resources don't need update permissions after setup - only read access. This is least-privilege security.

The initial setup (CloudFormation/Terraform) creates frozen resources and grants Alien the minimum permissions to manage live resources going forward.

## Complete Example

```typescript
import * as alien from "@alienplatform/core"

const storage = new alien.Storage("data-storage").build()

const fn = new alien.Function("processor")
  .code({ type: "source", toolchain: { type: "typescript" }, src: "." })
  .link(storage)
  .permissions("execution")
  .build()

export default new alien.Stack("my-stack")
  .add(storage, "frozen")
  .add(fn, "live")
  .permissions({
    profiles: {
      execution: {
        "data-storage": ["storage/data-read", "storage/data-write"],
      },
    },
  })
  .build()
```

