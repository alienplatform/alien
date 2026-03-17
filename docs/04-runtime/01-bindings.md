# Bindings

## What are Bindings?

Bindings connect Alien applications to resources. When an application needs to access a Storage bucket or a KV store, it uses bindings.

```rust
let ctx = AlienContext::from_env().await?;
let storage = ctx.bindings().load_storage("data-storage").await?;
storage.put(&"key".into(), bytes).await?;
```

The application doesn't know if `data-storage` is S3, Cloud Storage, or a local directory. The binding handles it.

## The Full Picture

Let's trace a binding from definition to use.

**Step 1: Developer defines resources**

```typescript
// alien.config.ts
const storage = new alien.Storage("data-storage").build()

const fn = new alien.Function("processor")
  .link(storage)  // <-- This creates the binding relationship
  .build()
```

**Step 2: alien-infra provisions Storage**

The `AwsStorageController` creates an S3 bucket. After creation, it generates binding params.

These bindings are **type-safe** and defined in `alien-core`:

```rust
// alien-core/src/bindings/storage.rs
#[derive(Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum StorageBinding {
    S3(S3StorageBinding),
    Gcs(GcsStorageBinding),
    Blob(BlobStorageBinding),
    Local(LocalStorageBinding),
}

#[derive(Serialize, Deserialize)]
pub struct S3StorageBinding {
    pub bucket_name: BindingValue<String>,
}
```

The controller uses this type-safe enum:

```rust
fn get_binding_params(&self) -> Option<serde_json::Value> {
    let binding = StorageBinding::s3(self.bucket_name.clone());
    serde_json::to_value(binding).ok()
}
// Result: {"service":"s3","bucketName":"myapp-data-storage-abc123"}
```

These binding params are stored in the resource's internal state (serialized in `StackState`), available for other resources that depend on it.

**Step 3: alien-infra provisions the compute resource**

Now `alien-infra` provisions the compute resource (Function, Container, Worker, etc.). It has `.link(storage)`, so it depends on the Storage resource.

Before creating the compute resource, the controller:
1. Looks at the resource's `links` to find dependencies
2. For each linked resource (`data-storage`), retrieves its binding params from that resource's internal state
3. Converts binding params to environment variables

The naming convention: `ALIEN_{RESOURCE_ID_UPPERCASE}_BINDING`

- Resource ID: `data-storage` → Environment variable: `ALIEN_DATA_STORAGE_BINDING`

The compute resource is created with these environment variables:

```
ALIEN_DEPLOYMENT_TYPE=aws
ALIEN_DATA_STORAGE_BINDING={"service":"s3","bucketName":"myapp-data-storage-abc123"}
```

At this point, provisioning is complete. The compute resource has the binding configuration as environment variables.

---

## Runtime Architecture

Now let's talk about what happens when the compute resource actually runs.

### The Problem

Alien applications can be written in Rust, TypeScript, or Python. The bindings implementation (S3 client, DynamoDB client, etc.) is in Rust. How do we let TypeScript and Python applications use these implementations?

We considered compiling `alien-bindings` to WASM or generating Python bindings via PyO3. While possible, we found gRPC to be much simpler. It also enables Alien applications to be written in *any* language or runtime that supports gRPC - not just the ones we explicitly target. If the need arises (e.g., for performance or for remote bindings), we can revisit WASM/PyO3 in the future.

### The Solution: Bindings Server and Client

We use a **server-client architecture** with gRPC:

- **Bindings Server**: `alien-runtime` - reads the binding environment variables, exposes bindings via gRPC
- **Bindings Client**: The Alien application - connects to the server via gRPC to use bindings

```
┌─────────────────────┐         gRPC          ┌─────────────────────┐
│  Alien Application  │ ◄──────────────────► │   alien-runtime     │
│  (Rust/TS/Python)   │                       │   (Bindings Server) │
└─────────────────────┘                       └──────────┬──────────┘
                                                         │
                                              ┌──────────▼──────────┐
                                              │  BindingsProvider   │
                                              │  (S3, DynamoDB...)  │
                                              └──────────┬──────────┘
                                                         │
                                              ┌──────────▼──────────┐
                                              │   Cloud Provider    │
                                              └─────────────────────┘
```

### How It Works

**Step 1: alien-runtime starts**

When the compute resource is invoked, `alien-runtime` starts first. It:

1. Reads `ALIEN_*_BINDING` environment variables
2. Starts a gRPC server (e.g., on `127.0.0.1:50051`)
3. Sets `ALIEN_BINDINGS_MODE=grpc` and `ALIEN_BINDINGS_GRPC_ADDRESS` environment variables
4. Starts the Alien application

**Step 2: Alien application connects**

The application reads `ALIEN_BINDINGS_MODE` and connects accordingly:

```rust
let ctx = AlienContext::from_env().await?;  // Detects mode, connects to gRPC if needed
let storage = ctx.bindings().load_storage("data-storage").await?;
storage.put(&"key".into(), bytes).await?;  // gRPC call → server → S3 API
```

When `ALIEN_BINDINGS_MODE=grpc`, every `storage.put()` call goes through gRPC to `alien-runtime`, which calls the actual S3 API.

### Bindings Mode

The `ALIEN_BINDINGS_MODE` environment variable controls how bindings are delivered:

- **`direct`** - Bindings loaded directly from environment variables. No gRPC server needed. Use this for:
  - Standalone processes (like deepstore-agent in the demo service)
  - Testing without alien-runtime
  - Any scenario where you have direct access to cloud credentials via env vars

- **`grpc`** - Bindings loaded via gRPC from alien-runtime. This is the standard mode when alien-runtime manages the application lifecycle.

Example configurations:

**Standalone process with AWS**:
```bash
ALIEN_BINDINGS_MODE=direct
ALIEN_DEPLOYMENT_TYPE=aws
ALIEN_DATA_BINDING='{"service":"s3","bucketName":"my-bucket"}'
AWS_ACCOUNT_ID=123456789
AWS_REGION=us-east-1
AWS_ACCESS_KEY_ID=...
AWS_SECRET_ACCESS_KEY=...
```

**Managed by alien-runtime**:
```bash
ALIEN_BINDINGS_MODE=grpc
ALIEN_BINDINGS_GRPC_ADDRESS=127.0.0.1:50051
ALIEN_DEPLOYMENT_TYPE=aws
# Bindings provided by alien-runtime via gRPC
```

The mode is independent of the platform. You can use `ALIEN_BINDINGS_MODE=direct` with any `ALIEN_DEPLOYMENT_TYPE` (aws, gcp, azure, local) as long as the appropriate credentials are available in environment variables.

---

## Detailed Components

Now let's look at each component in detail

## 1. Binding Definitions (alien-core)

Each resource type has a binding enum in `alien-core/src/bindings/`:

```rust
// alien-core/src/bindings/storage.rs
#[derive(Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum StorageBinding {
    S3(S3StorageBinding),
    Blob(BlobStorageBinding),
    Gcs(GcsStorageBinding),
    Local(LocalStorageBinding),
}

#[derive(Serialize, Deserialize)]
pub struct S3StorageBinding {
    pub bucket_name: BindingValue<String>,
}
```

The `service` tag determines which variant. When serialized:

```json
{
  "service": "s3",
  "bucketName": "my-app-data-storage-abc123"
}
```

Similar enums exist for `KvBinding`, `QueueBinding`, `VaultBinding`, `FunctionBinding`, etc.

## 2. Binding Generation (alien-infra)

Resource controllers generate binding params after provisioning. Each controller implements `get_binding_params()`:

```rust
// alien-infra/src/queue/aws.rs
impl AwsQueueController {
    fn get_binding_params(&self) -> Option<serde_json::Value> {
        if let Some(url) = &self.queue_url {
            let binding = QueueBinding::sqs(BindingValue::value(url.clone()));
            serde_json::to_value(binding).ok()
        } else {
            None
        }
    }
}
```

When the AWS Queue controller finishes provisioning an SQS queue, it creates a `QueueBinding::Sqs` with the queue URL. This is stored in the resource's internal state.

## 3. External Bindings

External bindings bypass provisioning entirely. Instead of creating resources, you provide pre-existing service details.

```yaml
# ExternalBindings (provided via Helm values or operator config)
storage:
  data-storage:
    service: s3
    bucketName: my-existing-bucket
    endpoint: https://minio.internal:9000
kv:
  cache:
    service: redis
    url: redis://redis.internal:6379
```

The executor handles external bindings in two phases:

**Phase 1: Skip provisioning**
- No controller is instantiated
- Resource marked as `Running` with `is_externally_provisioned: true`

**Phase 2: Inject bindings**
- When a compute resource links to an externally-bound resource
- Executor checks `external_bindings` first, then falls back to `controller.get_binding_params()`
- Binding injected as environment variable (same as controller-provisioned resources)

This works for any platform. You can use AWS for most resources but external Redis for KV.

### Kubernetes Secrets Support (SecretRef)

For Kubernetes deployments, sensitive binding fields can reference Kubernetes Secrets instead of embedding values directly in Helm values.

```yaml
infrastructure:
  cache:
    service: redis
    host: redis.internal
    port: 6379
    password:
      secretRef:
        name: redis-creds
        key: password
```

When the Kubernetes controller provisions workloads (Functions, Containers, Builds), it:

1. **Detects SecretRef objects** in binding JSON
2. **Extracts secrets** and creates individual environment variables with `valueFrom.secretKeyRef`
3. **Replaces SecretRef with placeholders** using `$(VAR)` syntax (Kubernetes environment variable expansion)
4. **Escapes existing `$(VAR)` patterns** in user values to prevent premature expansion

This keeps secrets out of pod specs while making them available at runtime. The workload receives:

```env
ALIEN_BINDING_CACHE_PASSWORD (from Secret: redis-creds.password)
ALIEN_CACHE_BINDING={"service":"redis","host":"redis.internal","port":6379,"password":"$(ALIEN_BINDING_CACHE_PASSWORD)"}
```

At pod runtime, Kubernetes expands `$(ALIEN_BINDING_CACHE_PASSWORD)` to the secret value from the Secret.

**Implementation**: See `alien-infra/src/core/k8s_secret_bindings.rs` for the extraction logic and `alien-infra/src/{function,container,build}/kubernetes.rs` for the controller integration.

## 4. Environment Variable Injection

When provisioning a compute resource (Function, Container, Worker), `alien-infra` collects binding params from linked resources and injects them as environment variables.

```rust
// alien-infra/src/core/environment_variables.rs
impl EnvironmentVariableBuilder {
    pub async fn add_linked_resources(
        mut self,
        links: &[ResourceRef],
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Self> {
        for link in links {
            let binding_name = link.id();
            let resource_state = ctx.state.resources.get(binding_name)?;
            
            if let Some(controller) = resource_state.get_internal_controller()? {
                if let Some(binding_params) = controller.get_binding_params() {
                    // Serialize binding as environment variable
                    let env_vars = serialize_binding_as_env_var(binding_name, &binding_params)?;
                    self.env_vars.extend(env_vars);
                }
            }
        }
        Ok(self)
    }
}
```

The function controller uses this when creating/updating Lambda, Cloud Run, etc.:

```rust
let env_vars = EnvironmentVariableBuilder::new(&function.environment)
    .add_standard_alien_env_vars(ctx)
    .add_linked_resources(&function.links, ctx, &function.id).await?
    .build();
```

Result: environment variables like:

```
ALIEN_DEPLOYMENT_TYPE=aws
ALIEN_DATA_STORAGE_BINDING={"service":"s3","bucketName":"my-app-data-storage-abc123"}
ALIEN_CACHE_BINDING={"service":"dynamodb","tableName":"my-app-cache-xyz789","region":"us-west-2"}
```

## 5. BindingsProvider

`BindingsProvider` routes to platform-specific implementations based on the binding's `service` tag:

```rust
// alien-bindings/src/provider.rs
impl BindingsProviderApi for BindingsProvider {
    async fn load_storage(&self, binding_name: &str) -> Result<Arc<dyn Storage>> {
        let binding: StorageBinding = self.get_binding(binding_name)?;
        
        match binding {
            StorageBinding::S3 { .. } => Ok(Arc::new(S3Storage::new(...))),
            StorageBinding::Gcs { .. } => Ok(Arc::new(GcsStorage::new(...))),
            StorageBinding::Blob { .. } => Ok(Arc::new(BlobStorage::new(...))),
            StorageBinding::Local { .. } => Ok(Arc::new(LocalStorage::new(...))),
        }
    }
}
```

### Local Platform

On local, bindings use local implementations:

| Binding | Cloud | Local |
|---------|-------|-------|
| Storage | S3, GCS, Blob Storage | Filesystem directory |
| KV | DynamoDB, Firestore, Table Storage | sled (embedded database) |
| Queue | SQS, Pub/Sub, Service Bus | File-backed queue |
| Vault | Secrets Manager, Key Vault | Encrypted local files |

## Remote Bindings

So far, bindings run inside the compute resource (Function, Container, etc.) via gRPC. But sometimes you need to access bindings from *outside* - from your own servers.

### Enabling Remote Access

By default, binding params stay local to prevent sensitive data in synced state. To enable remote access for a resource, set `remoteAccess: true`:

```typescript
// alien.config.ts
const customerStorage = new alien.Storage("customer-data").build()

export default new alien.Stack("my-stack")
  .add(customerStorage, "frozen", { remoteAccess: true })  // Binding params synced
  .add(internalDb, "frozen")                               // Binding params NOT synced
  .build()
```

```rust
// Rust equivalent
let stack = Stack::new("my-stack")
    .add_with_remote_access(storage, ResourceLifecycle::Frozen)  // Binding params synced
    .add(internal_db, ResourceLifecycle::Frozen)                 // Binding params NOT synced
    .build();
```

When `remoteAccess: true`, binding params are stored in `StackResourceState.remote_binding_params` and synced to the control plane. This enables external systems to access the resource.

### Use Case: Bring-Your-Own-Bucket

You're building a product (e.g., an observability platform). You want to store data in your customer's cloud - an S3 bucket in their AWS account - but you don't want to run code there.

Remote bindings enable this:

1. Alien provisions the storage in the customer's cloud (S3 on AWS, GCS on GCP, Blob Storage on Azure)
2. Your backend uses the bindings API to read/write to that bucket
3. Alien provides monitoring: tracks CloudTrail, monitors bucket size, cost inside the customer's cloud

```rust
// In your backend (runs in your cloud, not the customer's)
let provider = BindingsProvider::for_remote_deployment(deployment_id, token, None).await?;
let storage = provider.load_storage("customer-data").await?;

// Read/write to the customer's bucket - works regardless of which cloud it's in
storage.put(&"report.json".into(), report_data).await?;
```

One API. Works with S3, GCS, or Blob Storage. Alien handles the cloud-specific details.

> **Note:** Remote bindings are currently Rust-only. For backends written in TypeScript or Python, we need to provide SDK support. This is an open problem - there's no `alien-runtime` running, so the gRPC approach doesn't work here.

### Two Access Patterns

**Pattern 1: Direct Access** - You already have credentials and stack state.

```rust
let provider = BindingsProvider::from_stack_state(&stack_state, client_config)?;
let vault = provider.load_vault("secrets").await?;
vault.set_secret("API_KEY", "secret-value").await?;
```

Used by: `alien-deployment` (syncing secrets during deploy), control plane backends.

**Pattern 2: Remote Access** - You have a deployment ID and API token, but no credentials yet.

```rust
let provider = BindingsProvider::for_remote_deployment(deployment_id, token, None).await?;
let storage = provider.load_storage("data").await?;
storage.put(&"key".into(), data).await?;
```

Used by: developer backends (BYOB).

### Remote Access Flow

When you call `for_remote_deployment(deployment_id, token)`:

1. **Get deployment info** from the control plane API (`GET /deployments/{id}`)
   - Returns: `stackState`, `platform`, `managerId`

2. **Get credential resolver URL** from the control plane API
   - Returns: `url`

3. **Resolve credentials** from the credential resolver (`POST {url}/v1/deployment/resolve-credentials`)
   - Sends: `platform`, `stackState`
   - Returns: `clientConfig` (cloud credentials, e.g. temporary AWS token)

4. **All subsequent operations** go directly to the cloud provider (S3, GCS, etc.)
   - No proxying through the control plane

### Why This Design?

**Security:** The control plane handles authentication but never sees resolved cloud credentials. Credentials go directly from credential resolver → client → cloud provider.

**Scalability:** All resource operations (put, get, list) happen client-to-cloud. The control plane is not in the data path.

**Stateless:** Agent-manager receives stack state in the request. No need to store per-agent state.

## TypeScript and Python

TypeScript and Python SDKs are gRPC clients that connect to the same server:

```typescript
// TypeScript
import { AlienContext } from "@alienplatform/bindings"

const ctx = await AlienContext.fromEnv()
const storage = await ctx.bindings().loadStorage("data-storage")
await storage.put("key", data)
```

Same API. Same gRPC protocol. The Rust server handles platform-specific work.

## Summary

1. **alien-core** defines type-safe binding structs (`StorageBinding`, `KvBinding`, etc.)
2. **alien-infra** controllers generate binding params after provisioning resources
3. **alien-infra** injects bindings as `ALIEN_*_BINDING` env vars when provisioning compute resources
4. **alien-runtime** reads env vars, creates `BindingsProvider`, starts gRPC server
5. **Applications** connect via gRPC, use platform-agnostic APIs

