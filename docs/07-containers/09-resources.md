# Container Resources

This document describes the resources needed for Alien Containers.

## Summary

| Resource | User-Defined? | Auto-Generated? | Notes |
|----------|--------------|-----------------|-------|
| Network | ❌ | ✅ | Generated from `StackSettings.network` |
| ContainerCluster | ✅ | ❌ | User defines in `alien.ts` |
| Container | ✅ | ❌ | User defines in `alien.ts` |
| ServiceAccount | ❌ | ✅ | **Existing**. Generated from permission profiles |
| KubernetesNamespace | ❌ | ✅ | **Existing**. Generated for K8s platform |
| AzureResourceGroup | ❌ | ✅ | **Existing**. Generated for Azure platform |

---

## Network

See [NETWORK.md](../NETWORK.md) for full details.

**Generated automatically** from `StackSettings.network`. Users configure network in stack settings, not as a resource in `alien.ts`.

Three modes:
1. **None** — No network (valid for serverless-only stacks)
2. **Create** — Alien creates VPC/VNet with subnets
3. **BYO-VPC** — Use existing VPC/VNet (controller stores IDs)

**Key behaviors:**
- **Public subnets are derived**: Automatically included when any Function has `ingress: Public` or Container has a port with `public: true`
- **Outputs are cloud-agnostic**: For observability only. Resources needing VPC IDs use `require_dependency` to access controller state
- **BYO validation via preflights**: Runtime preflights validate BYO infrastructure exists before controller runs

**Platform behavior:**
- AWS: VPC, subnets, Internet Gateway, NAT Gateway, route tables
- GCP: VPC, subnets, Cloud NAT, firewall rules
- Azure: VNet, subnets, NAT Gateway, NSG
- Kubernetes: **No Network resource** — preflight doesn't generate it
- Local: **No Network resource** — preflight doesn't generate it

---

## ContainerCluster

**User-defined** in `alien.ts`. Represents the compute infrastructure that runs containers.

### Properties

```typescript
ContainerCluster({
  // Required
  id: string,
  serviceAccount: string,       // Reference to permission profile
  
  // Capacity groups
  capacityGroups: {
    [name: string]: {
      profile: "balanced" | "compute-optimized" | "memory-optimized" | 
               "storage-optimized" | "gpu-inference" | "gpu-training",
      minInstances: number,
      maxInstances: number,
      gpuType?: "nvidia-t4" | "nvidia-a10g" | "nvidia-a100" | ...,  // For GPU profiles
    }
  }
})
```

### Example

```typescript
ContainerCluster({
  id: "main",
  serviceAccount: "execution",
  capacityGroups: {
    general: {
      profile: "balanced",
      minInstances: 1,
      maxInstances: 10,
    },
    gpu: {
      profile: "gpu-inference",
      gpuType: "nvidia-t4",
      minInstances: 0,
      maxInstances: 3,
    },
  }
})
```

### Platform behavior

| Platform | Implementation |
|----------|----------------|
| AWS | Auto Scaling Groups, Launch Templates, horizond agents |
| GCP | Managed Instance Groups, Instance Templates, horizond agents |
| Azure | VM Scale Sets, horizond agents |
| Kubernetes | Configures StorageClasses; no compute infrastructure created |
| Local | Docker setup |

### Notes

- Uses **abstract profiles** instead of instance types — the platform maps to appropriate instances
- References a **ServiceAccount** (via permission profile) for cloud credentials
- Depends on **Network** on AWS/GCP/Azure (auto-added by preflight)

---

## Container

**User-defined** in `alien.ts`. Represents a container workload.

### Properties

```typescript
Container({
  // Required
  id: string,
  code: ContainerCode,          // Image or Source with toolchain
  permissions: string,          // Permission profile name
  cpu: ResourceSpec,            // { min: string, desired: string }
  memory: ResourceSpec,         // { min: string, desired: string }
  ports: ContainerPort[],       // At least one port required
  
  // Optional
  cluster?: string,             // Reference to ContainerCluster (auto-assigned if omitted)
  command?: string[],
  environment?: Record<string, string>,
  links?: ResourceRef[],
  
  // GPU
    gpu?: {
    gpuType: string,            // e.g., "nvidia-t4", "nvidia-a100"
      count: number,
    },
  
  // Storage
  ephemeralStorage?: string,    // e.g., "10Gi", "500Gi"
  persistentStorage?: PersistentStorage,  // Full object with size, mountPath, etc.
  
  // Scaling
  replicas?: number,            // Fixed replica count
  autoscaling?: ContainerAutoscaling,  // { min, desired, max, targets... }
  
  // Behavior
  stateful?: boolean,           // Default: false
  pool?: string,                // Capacity group assignment
  
  // Health
  healthCheck?: HealthCheck,    // { path, port?, method, timeoutSeconds, failureThreshold }
})
```

### Key Types

**ContainerCode:**
```typescript
// Pre-built or local image (always pulled and re-pushed for reproducibility)
{ type: "image", image: string }

// Source code to build
{ type: "source", src: string, toolchain: ToolchainConfig }
```

**ToolchainConfig:**
```typescript
{ type: "rust", binaryName: string }
{ type: "typescript", binaryName?: string }
{ type: "docker", dockerfile?: string, buildArgs?: Record<string, string>, target?: string }
```

**ContainerPort:**
```typescript
{
  port: number,
  expose?: {
    protocol: "http" | "tcp"  // "http" = HTTPS on 443, "tcp" = passthrough
  }
}
```

**PersistentStorage:**
```typescript
{
  size: string,              // e.g., "100Gi", "500Gi"
  mountPath: string,         // e.g., "/var/lib/postgresql/data"
  storageType?: string,      // e.g., "gp3", "pd-ssd"
  iops?: number,
  throughput?: number,
}
```

**ResourceSpec:**
```typescript
{
  min: string,               // Minimum resource allocation
  desired: string,           // Desired resource allocation
}
```

### Example

```typescript
Container({
  id: "api",
  cluster: "main",
  code: { type: "image", image: "my-app:latest" },
  permissions: "execution",
  cpu: { min: "2", desired: "2" },
  memory: { min: "4Gi", desired: "4Gi" },
  ports: [
    { port: 8080, expose: { protocol: "http" } },
    { port: 9090 },  // Internal only
  ],
  ephemeralStorage: "50Gi",
  persistentStorage: {
    size: "100Gi",
    mountPath: "/data",
  },
  stateful: true,
  replicas: 2,
  healthCheck: { path: "/health", port: 8080, method: "GET", timeoutSeconds: 1, failureThreshold: 3 },
})
```

### Platform behavior

| Platform | Implementation |
|----------|----------------|
| AWS/GCP/Azure | Horizon container scheduling |
| Kubernetes | `Deployment` (stateless) or `StatefulSet` (stateful) + `Service` + PVCs |
| Local | Docker container |

### Notes

- `stateful: true` means:
  - Sticky identity (stable hostname)
  - Ordered scaling
  - On K8s: Uses `StatefulSet` instead of `Deployment`
- `ephemeralStorage` uses local NVMe/SSD for performance
- `persistentStorage` requires `stateful: true` — uses PVCs on K8s, EBS/GCE PD/Azure Disk on VMs
- References a **ServiceAccount** (via permission profile) for cloud credentials

---

## Existing Resources (No Changes Needed)

### ServiceAccount

**Already implemented and production-grade.** Auto-generated from permission profiles via `ServiceAccountMutation`.

Trust policies include EC2/VM principals for container workloads. See [Cloud Identity](11-cloud-identity.md).

### KubernetesNamespace

**Already implemented.** Auto-generated for Kubernetes platform via `KubernetesNamespaceMutation`.

### AzureResourceGroup

**Already implemented.** Auto-generated for Azure platform via `AzureResourceGroupMutation`.

---

## Resource Dependencies

**AWS / GCP / Azure:**
```
Network (auto-generated from StackSettings)
    │
    └── ContainerCluster ──── ServiceAccount (auto-generated from profile)
            │
            └── Container (multiple)
```

**Kubernetes / Local:**
```
ContainerCluster ──── ServiceAccount (auto-generated from profile)
        │
        └── Container (multiple)
```
(No Network resource on these platforms)

---

## Implementation Order

1. **Network** — Resource definition, preflight mutation, controllers
2. **ContainerCluster** — Resource definition, controllers, Horizon integration
3. **Container** — Resource definition, controllers, K8s manifests
4. **ServiceAccount extension** — Add EC2/VM trust policies

