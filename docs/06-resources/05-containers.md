# Containers

Containers are always-on workloads — web servers, databases, background workers. Unlike Functions (which scale to zero), Containers run on dedicated VMs managed by Alien via Horizon.

```typescript
const api = new alien.Container("api")
  .code({ type: "source", toolchain: { type: "typescript" }, src: "./api" })
  .port(3000)
  .replicas(2)
  .build()

const db = new alien.Container("postgres")
  .code({ type: "image", image: "postgres:16" })
  .stateful(true)
  .persistentStorage("100Gi")
  .build()
```

Containers can be built from source (includes `alien-runtime` with full capabilities) or use pre-built images (plain container, no bindings/events/commands).

## Key Concepts

- **ContainerCluster**: The underlying VM infrastructure (Auto Scaling Group on AWS, Managed Instance Group on GCP, VMSS on Azure). Created automatically by preflights when any Container resource exists.
- **Horizon**: The container orchestration layer that schedules replicas onto machines, manages networking, and handles autoscaling.
- **Two-level scaling**: Horizon scales replicas; `alien-infra` scales machines in the cluster.

## Deep Dive

For architecture details, deployment flow, machine autoscaling, and platform-specific implementation, see [07-containers/](../07-containers/).
