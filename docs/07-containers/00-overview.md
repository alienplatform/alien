# Containers

Always-on containerized workloads with internal networking and automatic infrastructure management.

## Overview

Containers run on compute instances (EC2, GCE, Azure VMs) in remote environments. Alien handles infrastructure provisioning; Horizon handles container orchestration.

**What Alien handles:**
- Instance type selection
- Auto Scaling Groups
- Load balancers (ALB/NLB)
- Volumes (ephemeral and persistent)
- Machine autoscaling

**What Horizon handles:**
- Container scheduling
- Replica autoscaling
- Internal networking (WireGuard mesh)
- Service discovery (DNS)

## Contents

- **[Overview](1-overview.md)** — Container concepts and use cases
- **[Architecture](2-architecture.md)** — How Alien and Horizon work together
- **[Quickstart](2-quickstart.md)** — Deploy your first container
- **[Resource API](3-resource-api.md)** — Container configuration reference
- **[Infrastructure](4-infrastructure.md)** — ComputePools and instance selection
- **[Deployment Flow](5-deployment-flow.md)** — Step-by-step deployment process
- **[Machine Autoscaling](6-machine-autoscaling.md)** — How Alien scales VMs
- **[Local](7-local.md)** — Running containers locally with Docker
- **[Resources](8-resources.md)** — Additional resources
- **[Update Flow](10-update-flow.md)** — How config changes propagate during deployment
- **[Cloud Identity](11-cloud-identity.md)** — IMDS proxy, image pull auth, per-container SA impersonation

## Key Features

**Automatic Infrastructure:**
- You declare containers with resource requirements
- Alien selects optimal instance types
- Alien provisions Auto Scaling Groups
- No manual infrastructure configuration

**Container Orchestration:**
- Horizon schedules containers across machines
- WireGuard mesh for internal networking
- DNS service discovery (`.svc` domains)
- Replica autoscaling based on CPU/memory

**Public Networking:**
- Alien creates load balancers
- Horizon runs TCP proxies
- Automatic target registration
- Health checks and SSL support

**Stateful Workloads:**
- Stable ordinals (postgres-0, postgres-1, ...)
- Per-replica persistent volumes
- Zone pinning
- Ordered rollouts

**Cloud Identity:**
- Per-container service account impersonation via IMDS proxy
- Cross-account image pull from managing account registries
- Transparent credential vending — containers use standard cloud SDKs

**Two-Level Autoscaling:**
- Fast: Horizon scales replicas (5-30 seconds)
- Slow: Alien scales machines (3-4 minutes)

## Example Stack

```typescript
const storage = new alien.Storage("uploads").build()
const queue = new alien.Queue("jobs").build()

const api = new alien.Container("api")
  .code({ type: "source", toolchain: { type: "node" }, src: "./api" })
  .cpu(1).memory("2Gi")
  .minReplicas(2).maxReplicas(10)
  .port(3000)
  .expose("http")
  .link(storage)
  .link(queue)
  .build()

const worker = new alien.Container("worker")
  .code({ type: "source", toolchain: { type: "node" }, src: "./worker" })
  .cpu(2).memory("4Gi")
  .replicas(3)
  .link(storage)
  .link(queue)
  .build()

const postgres = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .cpu(4).memory("16Gi")
  .replicas(1)
  .stateful(true)
  .persistentStorage("500Gi")
  .port(5432)
  .build()

export default new alien.Stack("my-app")
  .add(storage, "frozen")
  .add(queue, "frozen")
  .add(api, "live")
  .add(worker, "live")
  .add(postgres, "live")
  .build()
```

## Containers vs Functions

See [Compute Resources](../00-compute-resources.md) for a detailed comparison of Function, Container, and Worker.

## Why Horizon Instead of Kubernetes?

Kubernetes adds complexity that doesn't fit BYOC scenarios:

- **Control plane overhead**: K8s requires a control plane in each customer account
- **Multi-tenant cost**: Separate control plane per customer is expensive
- **Operational burden**: K8s is hard to operate at small scale (≤10 machines)

Horizon provides container orchestration without these downsides:

- Only VMs running `horizond` in the customer account
- Control plane is fully managed (Cloudflare Durable Objects)
- Two components instead of 10+

