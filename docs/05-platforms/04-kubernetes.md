# Kubernetes Platform

Deploy applications to Kubernetes clusters using the pull model. Infrastructure resources (Storage, Queue, KV, etc.) are provided externally via bindings.

## When to Use

Use Kubernetes platform when:

**Cloud Kubernetes (EKS/GKE/AKS)**:
- Security team doesn't allow creating EC2/Lambda/Cloud Run directly
- Company has existing Kubernetes platform managed by DevOps/Platform team
- No cross-account access allowed (prefer Operator-only deployment)
- Standardizing all workloads on Kubernetes

**On-premises**:
- Self-hosted clusters with MinIO, Kafka, Redis
- No cloud account access

**Airgapped**:
- No internet access
- All services internal

**When to use AWS/GCP/Azure platforms instead**: You control the cloud account AND security policies allow creating compute resources (Lambda, EC2, Cloud Run) directly.

## Architecture

```
                                ┌────────────────┐
                                │ Control Plane  │
                                └────────────────┘
                                        ▲
                                        │ HTTPS (outbound only)
┌───────────────────────────────────────┼───────────────────────────────────────┐
│  Kubernetes Cluster (namespace-scoped)│                                       │
│  ┌────────────────────────────────────┼────────────────────────────────────┐  │
│  │  Namespace: acme-production        │                                    │  │
│  │                                    │                                    │  │
│  │  ┌─────────────────┐        ┌──────┴────────┐                          │  │
│  │  │ Operator Pod    │───────►│ Functions     │                          │  │
│  │  │ (Deployment)    │        │ Containers    │                          │  │
│  │  └─────────────────┘        │ Builds        │                          │  │
│  │                             └───────────────┘                          │  │
│  │  ┌─────────────────────────────────────────────┐                       │  │
│  │  │ ServiceAccounts (created by Helm)           │                       │  │
│  │  │ - acme-production-reader-sa (IRSA/WI)       │                       │  │
│  │  │ - acme-production-writer-sa (IRSA/WI)       │                       │  │
│  │  └─────────────────────────────────────────────┘                       │  │
│  │                                                                         │  │
│  └─────────────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────────────┘
                                        │
                                        │ external bindings
                                        ▼
┌───────────────────────────────────────────────────────────────────────────────┐
│  External Infrastructure (provided by customer)                               │
│                                                                               │
│  S3/MinIO    SQS/Kafka    Redis    PostgreSQL    Vault    OCI Registry        │
│                                                                               │
└───────────────────────────────────────────────────────────────────────────────┘
```

**Key principles**:
- **Namespace-scoped** — All resources created in one namespace
- **External infrastructure** — Operator does NOT provision Storage, Queue, KV, etc.
- **Outbound only** — Operator initiates connection to the control plane (pull model)
- **No cluster-admin** — Operator only needs permissions in its namespace

## What Gets Deployed

### By Helm Chart (one-time)

When installing the Helm chart:

```bash
helm install acme-monitoring \
  oci://public.ecr.aws/acme/acme-monitoring \
  --namespace production \
  --create-namespace \
  --values values.yaml
```

Helm creates:
- **Namespace** — `production` (from `--namespace`)
- **ServiceAccount** — For operator pod (RBAC)
- **ServiceAccounts** — One per permission profile (e.g., `reader`, `writer`) + `build-sa`
- **Role/RoleBinding** — RBAC permissions for operator
- **Secret** — Deployment token, encryption key
- **Deployment** — Operator pod (configuration via env vars)
- **Services** — For public Functions/Containers (internal DNS + load balancer backend)
- **Ingress** — For public resources with `type: ingress` in services config
- **NetworkPolicy** — For build job sandboxing (restricts network access)

**Application ServiceAccounts** (like `acme-monitoring-reader-sa`):
- Created by Helm templates based on stack analysis
- Annotated with cloud identity (IAM role ARN, GSA email, etc.)
- Referenced by Function/Container/Build pods

**Build ServiceAccount** (`acme-monitoring-build-sa`):
- Created automatically when stack contains Build resources
- Needs permissions to push to container registry
- Sandboxed via NetworkPolicy (can only access DNS and external HTTPS)

**Why Helm creates these**: They depend on `values.yaml` configuration (customer-provided hostnames, TLS certificates, cloud IAM roles) or stack analysis (which resources are public, whether builds exist). The Operator creates resources that change with each release.

### By Operator (dynamic)

The Operator watches for updates and deploys:

| Stack Resource | Kubernetes Resource | Notes |
|---------------|---------------------|-------|
| Function | Deployment | HTTP handler, event listener, or cron. Service created by Helm. |
| Container | Deployment or StatefulSet | Stateless: Deployment. Stateful: StatefulSet. Service created by Helm if public. |
| Build | Job | Builds and pushes images. ServiceAccount + NetworkPolicy created by Helm. |

**Operator does NOT create**:
- Storage, Queue, KV, Postgres, Vault resources (external bindings)
- ServiceAccounts (created by Helm based on stack analysis)
- Services/Ingress (created by Helm based on stack + values.yaml)
- NetworkPolicy (created by Helm for build jobs)
- Namespace (created by Helm)

## External Bindings

All infrastructure resources must be provided externally and configured in `values.yaml`.

### EKS with AWS Services

```yaml
management:
  token: dg_abc123...
  url: https://am.acme.com
  updates: auto
  telemetry: auto
  healthChecks: on

# Cloud identity mapping (standard K8s ServiceAccount fields)
serviceAccounts:
  reader:
    annotations:
      eks.amazonaws.com/role-arn: arn:aws:iam::123456789012:role/acme-reader
  writer:
    annotations:
      eks.amazonaws.com/role-arn: arn:aws:iam::123456789012:role/acme-writer

# External services
infrastructure:
  data-storage:
    service: s3
    bucketName: acme-data
    region: us-east-1
  
  event-queue:
    service: sqs
    queueUrl: https://sqs.us-east-1.amazonaws.com/123456789012/events
  
  cache:
    service: elasticache
    endpoint: cache.abc123.use1.cache.amazonaws.com
    port: 6379

# Public service exposure (optional)
services:
  api:
    type: ingress
    host: api.acme.com
    tls:
      certificateArn: arn:aws:acm:us-east-1:123456789012:certificate/abc
    ingress:
      className: alb
      annotations:
        alb.ingress.kubernetes.io/scheme: internet-facing
```

**IRSA (IAM Roles for Service Accounts)**:
- Helm creates K8s ServiceAccount with annotation: `eks.amazonaws.com/role-arn: arn:aws:iam::...`
- Function/Container pods use this ServiceAccount
- EKS injects AWS credentials automatically
- No static keys needed

### GKE with GCP Services

```yaml
management:
  token: dg_abc123...
  url: https://am.acme.com

serviceAccounts:
  reader:
    annotations:
      iam.gke.io/gcp-sa: acme-reader@my-project.iam.gserviceaccount.com

infrastructure:
  data-storage:
    service: gcs
    bucketName: acme-data
  
  event-queue:
    service: pubsub
    topicName: projects/my-project/topics/events

services:
  api:
    type: loadBalancer
```

**Workload Identity**:
- Helm creates K8s ServiceAccount with annotation: `iam.gke.io/gcp-sa: ...`
- GKE injects GCP credentials

### On-Premises with Self-Hosted Services

```yaml
management:
  token: dg_abc123...
  url: https://am.acme.com

# No serviceAccounts section - credentials in infrastructure

infrastructure:
  data-storage:
    service: minio
    endpoint: http://minio.internal:9000
    bucketName: acme-data
    accessKey: minioadmin
    secretKey: minioadmin
  
  event-queue:
    service: kafka
    brokers:
      - kafka-1.internal:9092
      - kafka-2.internal:9092
    topic: events
  
  cache:
    service: redis
    url: redis://:password@redis.internal:6379

services:
  api:
    type: clusterIp  # Customer manages external ingress
    publicUrl: https://api.acme.com  # For CORS, redirects
```

**Static Credentials**:
- Access keys/passwords in infrastructure config
- Stored in Kubernetes Secret (created by Helm)
- Injected as environment variables into pods

## Service Exposure

Public Functions and Containers are exposed via `services` configuration in `values.yaml`:

```yaml
services:
  api:
    type: ingress  # ingress | gateway | loadBalancer | clusterIp
    host: api.acme.com
    ingress:
      className: alb
```

Helm creates the networking resources (Ingress, Gateway, or Service). Application code accesses URLs via the Function and Container bindings:

- `Function::get_function_url()` — Returns public URL if available
- `Container::get_public_url()` — Returns public URL if exposed
- `Container::get_internal_url()` — Returns internal service URL

Helm charts handle detailed networking configuration, cloud-specific annotations, and URL resolution strategies.

## Deployment Flow

### 1. Developer Ships Update

```bash
alien deploy --project acme-monitoring
```

Control plane records new release.

### 2. Operator Syncs

Operator polls control plane every 30 seconds:

```
Operator: "Here's my current state"
Control plane: "Deploy v1.2.3"
```

### 3. Operator Deploys

Runs `alien-deployment::step()`:

```rust
// Get target from control plane sync
let target = sync_response.target_release;

// Controllers create/update Kubernetes resources
KubernetesFunctionController::apply(&api_function)?;
KubernetesFunctionController::apply(&worker_function)?;
KubernetesContainerController::apply(&background_service)?;
```

Each controller:
1. Reads desired state from stack
2. Reads current state from Kubernetes API
3. Computes diff
4. Applies changes (create/update/delete)

### 4. Function Deployment Example

For a Function resource in the stack:

**Operator creates**:
```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: acme-monitoring-api
  namespace: production
spec:
  template:
    spec:
      serviceAccountName: acme-monitoring-reader-sa  # References Helm-created SA
      containers:
      - name: function
        image: public.ecr.aws/acme/acme-monitoring:v1.2.3
        env:
        - name: ALIEN_DEPLOYMENT_TYPE
          value: kubernetes
        # External bindings injected as env vars
        - name: ALIEN_DATA_STORAGE_BINDING
          value: '{"service":"s3","bucketName":"acme-data","region":"us-east-1"}'
        - name: ALIEN_EVENT_QUEUE_BINDING
          value: '{"service":"sqs","queueUrl":"https://sqs...."}'
---
apiVersion: v1
kind: Service
metadata:
  name: acme-monitoring-api
spec:
  selector:
    app: acme-monitoring-api
  ports:
  - port: 80
    targetPort: 8080
```

**Function pod starts**:
- Uses ServiceAccount with IRSA annotation
- AWS credentials injected by EKS
- alien-runtime reads `ALIEN_*_BINDING` env vars
- Calls S3, SQS using injected credentials

## Update Handling

When resource configuration changes between deploys, the Operator runs Update handlers instead of Create handlers. Kubernetes handles rolling updates natively.

### Container Updates

**Update flow:** `UpdateStart → WaitingForUpdate → Ready`

The handler rebuilds the Deployment (or StatefulSet for stateful containers) spec with updated config — image, environment variables, resource requests/limits, volume mounts — then submits it via the Kubernetes API. It carries over the existing `resourceVersion` for optimistic concurrency.

After submitting, it polls readiness every 5 seconds (max 60 attempts = 5 minutes), checking `ready_replicas >= desired_replicas`. Kubernetes performs the rolling update automatically based on the Deployment's update strategy.

### Function Updates

**Update flow:** `UpdateStart → WaitingForUpdate → Ready`

Same pattern as Container — rebuilds and submits the Deployment, then polls readiness. Functions are always Deployments (never StatefulSets).

### Build Updates

**Update flow:** `DeletingOldJob → WaitingForOldJobDeletion → RecreatingJob → WaitingForRecreatedJob → Ready`

Kubernetes Jobs are immutable, so updates use a delete-then-recreate pattern. The handler deletes the old Job, waits for it to be gone (5 minutes timeout), creates a new Job with updated config, then waits for it to succeed (20 minutes timeout).

### Vault Updates

**Update flow:** `UpdateStart → Ready`

No-op handler — Vaults are immutable on Kubernetes. Individual secrets are created and verified on-demand during secret sync, not in the controller. This handler also recovers resources stuck in `RefreshFailed` state.

## Preflight Validation

Before deployment, Operator validates all infrastructure resources have external bindings:

```rust
// In alien-preflights
impl CompileTimeCheck for ExternalBindingsRequiredCheck {
    fn should_run(&self, platform: Platform) -> bool {
        platform == Platform::Kubernetes
    }

    async fn check(&self, stack: &Stack, external_bindings: &ExternalBindings) -> Result<CheckResult> {
        let infra_types = ["storage", "queue", "kv", "postgres", "vault"];
        
        let missing: Vec<_> = stack.resources
            .iter()
            .filter(|(id, entry)| {
                infra_types.contains(&entry.resource_type())
                    && !external_bindings.has(id)
            })
            .collect();

        if !missing.is_empty() {
            return Err(AlienError::new(ErrorData::MissingExternalBindings {
                resource_ids: missing
            }));
        }
        
        Ok(CheckResult::Passed)
    }
}
```

**Error if missing**:
```
Error: Missing external bindings for Kubernetes platform

The following resources require external bindings:
  - data-storage (Storage)
  - event-queue (Queue)

Add them to your Helm values.yaml:

  infrastructure:
    data-storage:
      service: s3
      bucketName: ...
```

## Lifecycle Enforcement

Infrastructure resources must be `Frozen` on Kubernetes. They cannot be provisioned dynamically.

**Compile-time check**:

```rust
impl CompileTimeCheck for KubernetesInfrastructureFrozenCheck {
    fn should_run(&self, stack: &Stack, platform: Platform) -> bool {
        platform == Platform::Kubernetes
    }

    async fn check(&self, stack: &Stack, platform: Platform) -> Result<CheckResult> {
        let infra_types = ["storage", "queue", "kv", "postgres", "vault"];
        
        let live_infra: Vec<_> = stack.resources
            .iter()
            .filter(|(_, entry)| {
                infra_types.contains(&entry.resource_type())
                    && entry.lifecycle == ResourceLifecycle::Live
            })
            .collect();

        if !live_infra.is_empty() {
            return Err(AlienError::new(ErrorData::InfrastructureMustBeFrozen {
                resource_ids: live_infra.iter().map(|(id, _)| id.clone()).collect()
            }));
        }
        
        Ok(CheckResult::Passed)
    }
}
```

**Error if violated**:
```
Error: Infrastructure resources must be Frozen on Kubernetes

The following resources are marked as Live but cannot be provisioned on Kubernetes:
  - data-storage (Storage)
  - cache (KV)

Change them to Frozen and provide external bindings in values.yaml.
```

## Secrets and Vault

Developer secrets (environment variables marked as `secret`) need a vault to store them. On Kubernetes, two options:

### Kubernetes Secrets (Default)

Uses native Kubernetes Secrets for developer secrets:

```yaml
infrastructure:
  secrets:
    service: kubernetes-secret
    # No additional config needed
```

Operator creates a Secret resource for each secret env var. Functions read via `ALIEN_SECRETS` env var pointing to the Secret.

### External Vault

For enterprises requiring HashiCorp Vault or cloud vaults:

```yaml
infrastructure:
  secrets:
    service: vault
    addr: https://vault.internal:8200
    path: secret/data/acme
    # Auth via serviceAccount or static token
  
  # OR cloud vault
  secrets:
    service: keyvault
    vaultUrl: https://acme-vault.vault.azure.net
```

See `01-provisioning/03-environment-variables.md` for how secrets flow from configuration to runtime.

## Container Storage

Containers can use ephemeral storage (caches, temp files) and persistent storage (databases, files).

**Ephemeral storage** — Lost on restart, uses local NVMe for performance:

```typescript
const search = new alien.Container("search")
  .ephemeralStorage("500Gi")  // Local SSD cache
```

**Persistent storage** — Survives restarts, uses PVCs:

```typescript
const db = new alien.Container("postgres")
  .stateful(true)
  .persistentStorage("100Gi")
```

Cloud-specific storage configuration (GKE Autopilot, AKS Container Storage, EKS instance store) is handled by Helm chart values.

## Updating External Services

When switching services (Redis A → Redis B), update `values.yaml`:

```yaml
infrastructure:
  cache:
    service: redis
    url: redis://new-redis.internal:6379  # Changed
```

Upgrade Helm release:

```bash
helm upgrade acme-monitoring acme/acme-monitoring -f values.yaml
```

Helm updates the Operator pod's environment variables:
1. Deployment template includes `checksum/config` annotation
2. Config change triggers pod restart with new env vars
3. Operator starts with updated `EXTERNAL_BINDINGS`
4. Next deployment step updates Function/Container Deployments
5. Kubernetes rolls out pods with new config

## Namespace Isolation

The Operator is namespace-scoped:

**Operator RBAC** (created by Helm):
```yaml
apiVersion: rbac.authorization.k8s.io/v1
kind: Role  # Not ClusterRole
metadata:
  name: acme-monitoring-operator
  namespace: production
rules:
- apiGroups: ["apps"]
  resources: ["deployments", "statefulsets"]
  verbs: ["create", "get", "list", "update", "delete"]
# ... more rules
---
apiVersion: rbac.authorization.k8s.io/v1
kind: RoleBinding  # Not ClusterRoleBinding
```

**Multi-tenancy**:
```bash
# Customer A
helm install acme-monitoring \
  --namespace customer-a \
  --values customer-a-values.yaml

# Customer B
helm install acme-monitoring \
  --namespace customer-b \
  --values customer-b-values.yaml
```

Each namespace is completely isolated. Different:
- ServiceAccounts
- External bindings
- Operator instances
- Resources

## Cloud Identity Mapping

Helm creates ServiceAccounts with cloud identity annotations. Pods using these ServiceAccounts get cloud credentials injected automatically.

| Platform | Annotation | Mechanism |
|----------|------------|-----------|
| **AWS/EKS** | `eks.amazonaws.com/role-arn: arn:aws:iam::...` | IRSA |
| **GCP/GKE** | `iam.gke.io/gcp-sa: ...@project.iam` | Workload Identity |
| **Azure/AKS** | `azure.workload.identity/client-id: ...` | Workload Identity |

Helm chart scenarios provide complete configuration examples per platform, including customer setup steps (IAM roles, trust policies, bindings).

## Implementation

| Component | Location | Purpose |
|-----------|----------|---------|
| Function controller | `alien-infra/src/function/kubernetes.rs` | Deployment + Service |
| Container controller | `alien-infra/src/container/kubernetes.rs` | Deployment/StatefulSet |
| Build controller | `alien-infra/src/build/kubernetes.rs` | Job |
| Helm chart generation | `packages-builder/src/builder/helm.rs` | Templates + values |
| Preflight check | `alien-preflights/src/compile_time/external_bindings_required.rs` | Validation |

**No ServiceAccount controller** — Created by Helm templates.

## Related Docs

- `01-provisioning/00-infra.md` — Frozen vs Live resources
- `04-runtime/01-bindings.md` — External bindings types

