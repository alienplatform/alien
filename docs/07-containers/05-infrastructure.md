# Containers - Infrastructure & Compute Pools

How Alien automatically provisions the right compute infrastructure for your containers.

## The Problem

When deploying containers, you need to answer: **what machines should run them?**

Kubernetes punts this problem to the user: "Define node pools, pick instance types, configure autoscaling groups." This works when you manage the infrastructure, but is problematic for BYOC where customers deploy to their own cloud accounts.

## Alien's Approach: Automatic Infrastructure

**You declare containers with resource requirements:**
```typescript
const api = new alien.Container("api")
  .cpu(1)
  .memory("2Gi")
  .maxReplicas(20)
```

**System automatically figures out:**
1. Total resources needed: 20 vCPU, 40 GB at max scale
2. Best instance type: `m7g.2xlarge` (8 vCPU, 32 GB, ARM)
3. Pool size: min=2, max=3 machines (with headroom)
4. Creates Auto Scaling Group with those settings

**You never manually configure:**
- Instance types
- Node pools
- Autoscaling groups
- Launch templates

## ComputePool Resource

The `ComputePool` is the infrastructure resource that manages compute instances.

```typescript
new alien.ComputePool(name: string)
  .instanceType(type: string)      // e.g., "m7g.2xlarge"
  .minSize(count: number)
  .maxSize(count: number)
  .gpu(config?: GpuConfig)
  .ephemeralStorage(size?: string)
  .build()
```

**Lifecycle:** `"frozen"` (created during initial setup, rarely changes)

## Automatic Pool Generation and Capacity Groups

By default, you **don't define ContainerClusters or capacity groups**. The system analyzes your containers and generates optimal infrastructure automatically during preflights.

**Input (what you write):**
```typescript
const api = new alien.Container("api")
  .cpu(1).memory("2Gi")
  .minReplicas(2).maxReplicas(20)
  .build()

export default new alien.Stack("my-app")
  .add(api, "live")
  .build()
```

**Output (what gets deployed):**
```typescript
// Generated automatically:
const computeCluster = new alien.ContainerCluster("compute")
  .groups([
    {
      groupId: "general",
      instanceType: "m7g.2xlarge",  // Auto-selected based on CPU:memory ratio
      minSize: 2,                    // HA requirement
      maxSize: 3,                    // Calculated: ceil((20 × 1 vCPU) / 8 vCPU per machine)
    }
  ])
  .build()

// Your original container (now assigned to capacity group):
const api = new alien.Container("api")
  .cpu(1).memory("2Gi")
  .minReplicas(2).maxReplicas(20)
  .capacityGroup("general")  // Auto-assigned
  .cluster(computeCluster)
  .build()

export default new alien.Stack("my-app")
  .add(computeCluster, "frozen")  // Generated
  .add(api, "live")               // Your original
  .build()
```

**Multiple containers with diverse resources:**

```typescript
// Input
const api = new alien.Container("api").cpu(1).memory("2Gi").maxReplicas(20)
const quickwit = new alien.Container("quickwit").cpu(4).memory("16Gi").ephemeralStorage("500Gi").maxReplicas(5)
const ml = new alien.Container("ml").gpu({ type: "nvidia-a100", count: 1 }).maxReplicas(2)

// Generated automatically
const computeCluster = new alien.ContainerCluster("compute")
  .groups([
    { groupId: "general", instanceType: "m7g.2xlarge" },      // For api
    { groupId: "storage", instanceType: "i4i.2xlarge" },      // For quickwit (NVMe)
    { groupId: "gpu", instanceType: "p4d.24xlarge" }          // For ml
  ])

// Auto-assigned:
// api → "general"
// quickwit → "storage"  
// ml → "gpu"
```

**Smart packing for small workloads:**

```typescript
// Input: 1 small API + 1 GPU workload
const api = new alien.Container("api").cpu(2).memory("4Gi").replicas(1)  // Only 1 replica!
const ml = new alien.Container("ml").gpu({ type: "nvidia-a100", count: 1 })

// Generated: Only GPU group (pack both on same expensive machines)
const computeCluster = new alien.ContainerCluster("compute")
  .groups([
    { groupId: "gpu", instanceType: "p4d.24xlarge" }
    // No separate "general" group - API fits on GPU machine
  ])

// api → "gpu" (packed with ml)
// ml → "gpu"
```

This happens during the **preflights phase** (see [ALIEN_PREFLIGHTS.md](../ALIEN_PREFLIGHTS.md)).

## Instance Type Selection Algorithm

The system picks instance types based on workload characteristics.

### Step 1: Categorize Containers into Capacity Groups

Containers are analyzed and assigned to capacity groups based on their requirements and scaling behavior:

```rust
enum GroupCategory {
    General,                    // CPU/memory balanced workloads
    Storage,                    // High ephemeral storage (>200Gi)
    Gpu(GpuType),              // GPU workloads (one group per GPU type)
}

fn categorize_and_assign_groups(containers: &[Container]) -> CapacityGroupPlan {
    let mut groups = HashMap::new();
    
    for container in containers {
        let category = if container.gpu.is_some() {
            // GPU container: one group per GPU type
            GroupCategory::Gpu(container.gpu.type)
        } else if container.ephemeral_storage > 200GB {
            // Large ephemeral storage needs NVMe instances
            GroupCategory::Storage
        } else {
            // Default: general-purpose compute
            GroupCategory::General
        };
        
        // Check if we should create dedicated group or pack with existing
        let group_id = if should_create_dedicated_group(container, category) {
            // Large scaling workloads get dedicated groups
            category.to_group_id()
        } else {
            // Small workloads can pack with other groups
            find_group_to_pack_with(container, &groups, category)
        };
        
        groups.entry(group_id)
            .or_insert_with(Vec::new)
            .push(container.name.clone());
    }
    
    groups
}

fn should_create_dedicated_group(container: &Container, category: GroupCategory) -> bool {
    match category {
        GroupCategory::Gpu(_) => true,  // Always dedicated (expensive)
        GroupCategory::Storage => true, // Always dedicated (specialized)
        GroupCategory::General => {
            // Create dedicated group if will scale significantly
            let max_replicas = container.max_replicas.unwrap_or(10);
            max_replicas > 10  // Threshold: >10 replicas needs own group
        }
    }
}
```

**Example:**
```typescript
// Scenario 1: Diverse scaling workloads
const api = new alien.Container("api").cpu(1).memory("2Gi").maxReplicas(50)
const quickwit = new alien.Container("quickwit").cpu(4).memory("16Gi").ephemeralStorage("500Gi").maxReplicas(5)
const ml = new alien.Container("ml").gpu({ type: "nvidia-a100", count: 1 })

// Result: 3 separate groups
// - "general": [api] (scales significantly, needs dedicated group)
// - "storage": [quickwit] (specialized hardware)
// - "gpu": [ml] (specialized hardware)
```

```typescript
// Scenario 2: Small workload + GPU
const api = new alien.Container("api").cpu(2).memory("4Gi").replicas(1)  // Fixed 1 replica
const ml = new alien.Container("ml").gpu({ type: "nvidia-a100", count: 1 })

// Result: 1 group (pack both on expensive GPU machines)
// - "gpu": [api, ml]
```

**Note:** One ContainerCluster can have multiple capacity groups, each backed by its own ASG. All machines join the same Horizon cluster for unified networking.

### Step 2: Select Instance Family

For General and Storage pools, analyze the **CPU:memory ratio**:

```rust
fn select_instance_family(containers: &[Container]) -> InstanceFamily {
    let total_cpu: f32 = containers.iter()
        .map(|c| c.cpu * c.max_replicas as f32)
        .sum();

    let total_memory_gb: f32 = containers.iter()
        .map(|c| parse_gb(&c.memory) * c.max_replicas as f32)
        .sum();
    
    let cpu_memory_ratio = total_cpu / total_memory_gb;
    
    // Special case: tiny workloads use burstable
    if total_cpu < 2.0 {
        return InstanceFamily::Burstable;  // t4g, e2-micro, B-series
    }
    
    // Match ratio to instance family
    if cpu_memory_ratio > 0.35 {
        InstanceFamily::ComputeOptimized   // c7g, c3-standard, F-series
    } else if cpu_memory_ratio < 0.18 {
        InstanceFamily::MemoryOptimized    // r7g, m2-megamem, E-series
    } else {
        InstanceFamily::GeneralPurpose     // m7g, n2-standard, D-series
    }
}
```

**Instance Family Ratios:**

| Ratio | vCPU:Memory | Family | AWS Example | Use Case |
|-------|-------------|--------|-------------|----------|
| **> 0.35** | 1:2 or higher | Compute-Optimized | c7g.2xlarge (8 vCPU, 16 GB) | Video encoding, compilation |
| **0.18-0.35** | ~1:4 | General-Purpose | m7g.2xlarge (8 vCPU, 32 GB) | Web APIs, most workloads |
| **< 0.18** | 1:8 or lower | Memory-Optimized | r7g.2xlarge (8 vCPU, 64 GB) | Redis, in-memory caching |
| **< 2 vCPU total** | Any | Burstable | t4g.medium (2 vCPU, 4 GB) | Dev/test, low traffic |

**Examples:**

```typescript
// Service: 1 vCPU, 2 GB → ratio = 0.5 → Compute-Optimized
const encoder = new alien.Container("encoder")
  .cpu(1).memory("2Gi").maxReplicas(10)
// → c7g instances (more CPU per dollar)

// Service: 1 vCPU, 4 GB → ratio = 0.25 → General-Purpose  
const api = new alien.Container("api")
  .cpu(1).memory("4Gi").maxReplicas(20)
// → m7g instances (balanced)

// Service: 1 vCPU, 8 GB → ratio = 0.125 → Memory-Optimized
const cache = new alien.Container("cache")
  .cpu(1).memory("8Gi").maxReplicas(5)
// → r7g instances (more memory per dollar)

// Service: 0.25 vCPU, 512 MB, max 3 → total < 2 vCPU → Burstable
const cron = new alien.Container("cron")
  .cpu(0.25).memory("512Mi").maxReplicas(3)
// → t4g instances (cheapest for small workloads)
```

### Step 3: Select Specific Instance Type

Within a family, pick the instance that best fits:

```rust
fn select_instance_type(
    family: InstanceFamily,
    total_cpu: f32,
    total_memory_gb: f32,
    deployment_size: DeploymentSize,
) -> String {
    // Apply headroom based on deployment size
    let headroom = match deployment_size {
        Small => 1.1,    // 10% extra
        Medium => 1.25,  // 25% extra
        Large => 1.5,    // 50% extra
        XLarge => 2.0,   // 100% extra
    };
    
    let cpu_with_headroom = total_cpu * headroom;
    let memory_with_headroom = total_memory_gb * headroom;
    
    // Target: fit workload on ~3 machines (good bin-packing)
    let target_cpu_per_machine = cpu_with_headroom / 3.0;
    let target_memory_per_machine = memory_with_headroom / 3.0;
    
    // Get instances in this family
    let catalog = get_instance_catalog();
    let candidates = catalog.filter_by_family(family);
    
    // Find smallest instance that meets requirements
    candidates.iter()
        .filter(|i| {
            i.vcpu >= target_cpu_per_machine &&
            i.memory_gb >= target_memory_per_machine
        })
        .min_by_key(|i| i.vcpu)
        .unwrap()
}
```

**Example calculation:**

```typescript
// Service: 1 vCPU, 2 GB, maxReplicas: 20
// Deploy: --size medium

// Step 1: Calculate total at max
total_cpu = 1.0 * 20 = 20.0 vCPU
total_memory = 2.0 * 20 = 40.0 GB

// Step 2: Apply headroom (medium = 1.25x)
cpu_with_headroom = 20.0 * 1.25 = 25.0 vCPU
memory_with_headroom = 40.0 * 1.25 = 50.0 GB

// Step 3: Target per machine (~3 machines)
target_cpu = 25.0 / 3 = 8.33 vCPU
target_memory = 50.0 / 3 = 16.67 GB

// Step 4: Find smallest instance that fits
// AWS General-Purpose ARM instances:
//   m7g.large:   2 vCPU,  8 GB  ❌ (too small)
//   m7g.xlarge:  4 vCPU, 16 GB  ❌ (too small)
//   m7g.2xlarge: 8 vCPU, 32 GB  ✓ (fits!)

// Result: m7g.2xlarge
```

### Step 4: Calculate Pool Size

```rust
fn calculate_pool_size(
    instance_vcpu: f32,
    total_cpu_with_headroom: f32,
    deployment_size: DeploymentSize,
) -> (usize, usize) {
    // Max instances: fit total CPU
    let max_instances = (total_cpu_with_headroom / instance_vcpu).ceil();
    
    // Min instances: HA requirement based on size
    let min_instances = match deployment_size {
        Small => 1,   // Single AZ, no HA
        Medium => 2,  // 2 AZs, basic HA
        Large => 3,   // 3 AZs, full HA
        XLarge => 4,  // 3 AZs + 1 spare
    };
    
    (min_instances, max_instances.max(min_instances))
}
```

**Continuing the example:**
```
total_cpu_with_headroom = 25.0 vCPU
instance_vcpu = 8.0 (m7g.2xlarge)
deployment_size = Medium

max_instances = ceil(25.0 / 8.0) = 4 machines
min_instances = 2 (medium HA requirement)

Result: min=2, max=4
```

## Deployment Size

The `--size` flag controls capacity and HA:

```bash
alien deploy prod --size small|medium|large|xlarge
```

| Size | Headroom | Min Instances | AZ Distribution | Use Case |
|------|----------|---------------|-----------------|----------|
| **small** | +10% | 1 | 1 AZ | Dev/test, cost-sensitive |
| **medium** | +25% | 2 | 2 AZs | Staging, small production |
| **large** | +50% | 3 | 3 AZs | Production, HA required |
| **xlarge** | +100% | 4 | 3 AZs + spare | Critical, max availability |

**Headroom** is extra capacity beyond max replicas for:
- Burst traffic handling
- Rolling updates (old + new containers)
- Smooth autoscaling

## Instance Type Catalog

The system maintains a catalog of instance types per cloud.

### AWS (ARM preferred for cost efficiency)

| Family | Example | vCPU | Memory | Cost/hr | Use Case |
|--------|---------|------|--------|---------|----------|
| **Burstable** | t4g.medium | 2 | 4 GB | $0.034 | Small workloads |
| **General** | m7g.2xlarge | 8 | 32 GB | $0.326 | Most workloads |
| **Compute** | c7g.2xlarge | 8 | 16 GB | $0.145 | CPU-intensive |
| **Memory** | r7g.2xlarge | 8 | 64 GB | $0.302 | Memory-intensive |
| **Storage** | i4i.2xlarge | 8 | 64 GB | $0.768 | NVMe workloads |
| **GPU A100** | p4d.24xlarge | 96 | 1152 GB | $32.77 | ML training |

**Why ARM?** ARM instances (t4g, m7g, c7g, r7g) are ~20% cheaper than x86 equivalents with comparable performance.

### GCP

| Family | Example | vCPU | Memory | Use Case |
|--------|---------|------|--------|----------|
| **Burstable** | e2-micro | 0.25 | 1 GB | Tiny workloads |
| **General** | n2-standard-8 | 8 | 32 GB | Most workloads |
| **Compute** | c3-standard-8 | 8 | 16 GB | CPU-intensive |
| **Memory** | m2-megamem-416 | 8 | 64 GB | Memory-intensive |
| **Storage** | c3d-standard-8 | 8 | 32 GB + 480GB NVMe | NVMe workloads |
| **GPU A100** | a2-highgpu-8g | 96 | 1360 GB | ML training |

### Azure

| Family | Example | vCPU | Memory | Use Case |
|--------|---------|------|--------|----------|
| **Burstable** | Standard_B1s | 1 | 1 GB | Tiny workloads |
| **General** | Standard_D8s_v5 | 8 | 32 GB | Most workloads |
| **Compute** | Standard_F8s_v2 | 8 | 16 GB | CPU-intensive |
| **Memory** | Standard_E8s_v5 | 8 | 64 GB | Memory-intensive |
| **Storage** | Standard_L8s_v3 | 8 | 64 GB + 1.9TB NVMe | NVMe workloads |
| **GPU A100** | NC96ads_A100_v4 | 96 | 880 GB | ML training |

## GPU Instance Selection

GPU services get dedicated pools by GPU type:

```rust
fn select_gpu_instance(gpu_type: GpuType, platform: Platform) -> String {
    match (platform, gpu_type) {
        (Aws, NvidiaT4) => "g4dn.xlarge",        // 1x T4 (16GB)
        (Aws, NvidiaA100) => "p4d.24xlarge",     // 8x A100 (40GB)
        (Aws, NvidiaH100) => "p5.48xlarge",      // 8x H100 (80GB)
        
        (Gcp, NvidiaT4) => "n1-standard-4" + T4, // 1x T4 attached
        (Gcp, NvidiaA100) => "a2-highgpu-1g",    // 1x A100 (40GB)
        (Gcp, NvidiaH100) => "a3-highgpu-8g",    // 8x H100 (80GB)
        
        (Azure, NvidiaT4) => "Standard_NC4as_T4_v3",        // 1x T4
        (Azure, NvidiaA100) => "Standard_NC24ads_A100_v4",  // 1x A100
        (Azure, NvidiaH100) => "Standard_ND96isr_H100_v5",  // 8x H100
    }
}
```

**Example:**
```typescript
const inference = new alien.Container("inference")
  .gpu({ type: "nvidia-a100", count: 1 })
  .minReplicas(1).maxReplicas(4)

// Creates pool:
//   AWS: p4d.24xlarge (8 GPUs, can fit 8 replicas)
//   min=2 (HA), max=1 (4 replicas ÷ 8 GPUs = 1 machine)
```

## Storage-Optimized Instance Selection

For services with large ephemeral storage (>200GB):

```typescript
const quickwit = new alien.Container("quickwit")
  .cpu(4).memory("16Gi")
  .ephemeralStorage("500Gi")  // Triggers storage-optimized pool
  .minReplicas(3).maxReplicas(12)
```

**Selection:**
```rust
fn select_storage_instance(
    max_ephemeral_gb: u64,
    total_cpu: f32,
    platform: Platform,
) -> String {
    let catalog = get_instance_catalog(platform);
    
    // Filter: instances with NVMe >= required
    let candidates = catalog.instances.iter()
        .filter(|i| {
            i.family == StorageOptimized &&
            i.nvme_gb.unwrap_or(0) >= max_ephemeral_gb
        });
    
    // Pick based on CPU needs
    let target_cpu = total_cpu / 3.0;
    
    candidates
        .filter(|i| i.vcpu >= target_cpu)
        .min_by_key(|i| i.vcpu)
        .unwrap()
}
```

**AWS storage-optimized instances:**

| Instance | vCPU | Memory | NVMe | Cost/hr |
|----------|------|--------|------|---------|
| i4i.xlarge | 4 | 16 GB | 937 GB | $0.384 |
| i4i.2xlarge | 8 | 64 GB | 1,875 GB | $0.768 |
| i4i.4xlarge | 16 | 128 GB | 3,750 GB | $1.536 |
| i4i.8xlarge | 32 | 256 GB | 7,500 GB | $3.072 |

## Manual Pool Definition

For fine control, you can define pools explicitly:

```typescript
const gpuPool = new alien.ComputePool("gpu")
  .gpu({ type: "nvidia-a100" })
  .minSize(2)
  .maxSize(4)
  .build()

const inference = new alien.Container("inference")
  .gpu({ type: "nvidia-a100", count: 1 })
  .pool(gpuPool)  // Explicit assignment
  .build()

export default new alien.Stack("ml-app")
  .add(gpuPool, "frozen")
  .add(inference, "live")
  .build()
```

**When to use manual pools:**
- Specific instance type requirements
- Shared pools across multiple containers
- Custom min/max sizing

## Cost Examples

### Small Enterprise (Analytics Dashboard)

```typescript
// 2 services: dashboard (0.25 vCPU, 512 MB, max 3)
//            query (0.5 vCPU, 1 GB, max 2)

// Total: 1.75 vCPU, 3.5 GB at max
// Ratio: 0.5 → Compute-Optimized? No, < 2 vCPU → Burstable
// Instance: t4g.medium (2 vCPU, 4 GB, $0.034/hr)
// Pool: min=1, max=1 machine

// Cost: $24/month
```

### Mid-Sized (Document Management)

```typescript
// Web API: 1 vCPU, 4 GB, max 10 replicas
// PDF processor: 2 vCPU, 4 GB, max 5 replicas

// Total: 20 vCPU, 60 GB at max
// Ratio: 0.33 → General-Purpose
// Instance: m7g.2xlarge (8 vCPU, 32 GB, $0.326/hr)
// Pool (medium): min=2, max=4 machines

// Cost: 
//   Baseline: 2 × $0.326 × 730 = $476/month
//   Peak: 4 × $0.326 × 730 = $952/month
```

### Large Enterprise (CRM Platform)

```typescript
// API: 2 vCPU, 4 GB, max 50 replicas
// Email worker: 4 vCPU, 8 GB, max 20 replicas
// Session cache: 1 vCPU, 8 GB, max 6 replicas

// Total: 186 vCPU, 408 GB at max
// Ratio: 0.456 → Compute-Optimized
// Instance: c7g.16xlarge (64 vCPU, 128 GB, $1.16/hr)
// Pool (large): min=3, max=5 machines

// Cost:
//   Baseline: 3 × $1.16 × 730 = $2,540/month
//   Peak: 5 × $1.16 × 730 = $4,234/month
```

## Next Steps

- **[Storage](5-storage.md)** - Ephemeral and persistent storage semantics
- **[Deployment Flow](7-deployment-flow.md)** - How infrastructure gets provisioned
- **[Autoscaling](8-autoscaling.md)** - How infrastructure scales with load
- **[Advanced Topics](11-advanced.md)** - Multi-pool setups, custom configurations

