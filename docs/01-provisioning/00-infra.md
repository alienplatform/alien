# Infrastructure Provisioning

## What is alien-infra?

In the previous doc, you saw how a Stack is defined: a list of Resources to deploy to a remote environment.

`alien-infra` is the Rust library that takes that Stack and actually makes it real — creating S3 buckets, Lambda functions, KV tables, container clusters, and everything else the Stack describes.

But it's not provision-and-done. `alien-infra` runs **continuously** throughout the lifetime of a deployment:

- **Initial provisioning** — creates frozen resources (buckets, vaults, VPCs)
- **Code deploys** — updates live resources (functions, containers) when new versions ship
- **Heartbeats** — periodically checks that resources still exist and are healthy
- **Reconciliation** — if something drifts (a resource is deleted externally), brings it back

All of this happens through one function: `step()`.

```rust
let result = executor.step(current_state).await?;
let next_state = result.next_state;
```

`step()` looks at the current state of every resource, picks the next piece of work to do, does it, and returns the updated state. The caller saves the state and calls `step()` again. This loop keeps running for as long as the deployment is alive.

## Why Build It?

You might ask: why not compile Stack definitions to Terraform?

### Two Execution Contexts

Terraform runs to completion from one place. Alien splits deployment across two phases that can run from different places.

**Initial Setup:**
- Creates IAM roles, VPCs, storage buckets
- Requires elevated permissions (often admin-level)
- Runs once, typically from the user's machine or via their Terraform

**Ongoing Operations:**
- Developer ships new updates, deploys new code
- Requires only least-privilege permissions
- Can run remotely or locally, push or pull:
  - **Remote (push):** alien-manager in the developer's cloud pushes updates via cross-account access
  - **Local (pull):** Agent inside the customer's Kubernetes cluster, or a daemon on their machine, pulls and applies updates

A typical flow: User runs `terraform apply` to create the initial infrastructure. Terraform exits. Later, a separate process (a Kubernetes controller, a local daemon, or a remote orchestrator) picks up and deploys application code. Different process, different machine, different permissions.

Terraform can't do this. It holds state in memory and must complete before exiting. You can't run `terraform apply` on your laptop and have a different process finish the deployment.

`alien-infra` is built for handoffs. Each `step()` returns serializable state. Store it anywhere. Resume from any process.

This is why frozen/live matters:

- **Frozen resources** → deployed during initial setup
- **Live resources** → deployed during ongoing operations

The executor filters by lifecycle. Same library, different phases, different permissions.

### Embeddability

Because `alien-infra` is a Rust library (not a CLI), it can be embedded anywhere:

- **Local deployments** — `alien dev` runs `alien-infra` on your laptop for local development, or on customer machines for local LLMs, endpoint security agents, robot fleets.

- **Serverless functions** - Run `step()` in Lambda or Cloud Run. No long-running processes.

- **"Login with Google" flows** - When a user clicks OAuth, we need fast deployment. `alien-infra` runs directly, no subprocess spawning.

- **Inside Terraform providers** - We embed `alien-infra` inside a custom Terraform provider. Every `step()` runs inside `terraform apply`. The platform sees every step - even during the customer's initial setup.

- **Kubernetes agents** - Run as an in-cluster controller managing resources via the K8s API.

### Scale

Terraform was built for O(1) environments - production, staging, maybe a few feature branches.

Alien serves millions of deployments. AI coding platforms like Lovable let users deploy apps to their own cloud accounts. Each user might have multiple deployments.

Terraform runs one process per deployment. Each process waits 10-60+ seconds for cloud APIs, holding state in memory. At this scale, you pay for compute that sits idle.

`alien-infra` is step-based. Each `step()` does one unit of work and returns. Workers pull steps from a queue. Horizontal scaling is trivial. No idle compute.

### Local Platform

Terraform has no concept of local provisioning. `alien-infra` does:

- **Local development** - Fast iterations via `alien dev` without deploying to the cloud
- **Local LLMs** - Download models (often GBs), update them, delete old versions. Model management is expensive and stateful.
- **Endpoint agents** - Security agents on employee laptops
- **Robot fleets** - Agents on Linux-based robots

## The Step Function

The entire library centers on one function:

```rust
pub async fn step(&self, state: StackState) -> Result<StepResult>
```

It takes the current state, performs work, returns the next state.

```rust
pub struct StepResult {
    pub next_state: StackState,
    pub suggested_delay_ms: Option<u64>,
}
```

`suggested_delay_ms` tells the caller how long to wait before the next call. Some operations require polling (e.g., waiting for a Lambda to become active after creation). Others can proceed immediately.

### StackState

```rust
pub struct StackState {
    pub platform: Platform,
    pub resource_prefix: String,
    pub resources: HashMap<String, StackResourceState>,
    pub settings: StackSettings,
}
```

Each resource tracks its own state:

```rust
pub struct StackResourceState {
    pub status: ResourceStatus,           // Pending, Provisioning, Running, etc.
    pub config: Resource,                 // Desired configuration
    pub outputs: Option<ResourceOutputs>, // ARN, URL, etc.
    pub internal_state: Option<Value>,    // Controller state (serialized)
}
```

The entire `StackState` is serializable. Store it between steps. Resume from any process.

**Important:** `StackState` is synced to the control plane. Any data in `StackState` flows through the network and is stored persistently. This includes `internal_state` from controllers. Never store secrets in controller state — only identifiers like bucket names, ARNs, and queue URLs.

### StackSettings

`StackSettings` customizes deployment behavior per environment. It's part of `StackState`.

```rust
pub struct StackSettings {
    pub network: Option<NetworkSettings>,  // VPC/VNet configuration
    pub domains: Option<DomainSettings>,   // Domain configuration (future)
}
```

Examples of what StackSettings controls:
- Create a new VPC or use an existing one?
- Auto-generated domain or custom domain?

These are user-customizable per deployment. Different customers deploying the same stack can make different choices.

### ManagementConfig

Controllers also need `ManagementConfig` to generate cross-account IAM policies. It's stored separately in `StackState.management_config` (not part of `settings`).

```rust
pub struct StackState {
    pub settings: StackSettings,                    // User-customizable
    pub management_config: Option<ManagementConfig>, // Platform-managed
    // ...
}
```

ManagementConfig contains identifiers (role ARNs, service account emails) - actual credentials are obtained via platform mechanisms like AssumeRole.

**Push mode:** Derived from the managing account's ServiceAccount resource  
**Pull mode:** `None` (Agent uses local credentials)

## Resource Controllers

Each resource type has a **controller** - a state machine that knows how to create, update, and delete that resource.

```rust
#[controller]
pub struct AwsStorageController {
    pub(crate) bucket_name: Option<String>,
}
```

Controllers define **handlers** for each state. Each handler performs **one mutable operation**:

```rust
#[controller]
impl AwsStorageController {
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // One mutable operation: create the bucket
        client.create_bucket(&bucket_name).await?;
        self.bucket_name = Some(bucket_name);
        
        Ok(HandlerAction::Continue {
            state: ConfiguringVersioning,
            suggested_delay: None,
        })
    }
    
    #[handler(state = ConfiguringVersioning, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn configuring_versioning(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // One mutable operation: configure versioning
        client.put_bucket_versioning(&bucket_name, enabled).await?;
        
        Ok(HandlerAction::Continue {
            state: ConfiguringLifecycle,
            suggested_delay: None,
        })
    }
    
    terminal_state!(Ready, ResourceStatus::Running);
    terminal_state!(CreateFailed, ResourceStatus::ProvisionFailed);
}
```

### Why One Operation Per State?

If a handler fails, the executor retries from that state. If you do multiple operations in one handler, you can't retry just the failed one.

### State Flow

Creation:
```
CreateStart → ConfiguringVersioning → ConfiguringLifecycle → Ready
```

Update:
```
UpdateStart → UpdatingVersioning → UpdatingLifecycle → Ready
```

Deletion:
```
DeleteStart → EmptyingBucket → DeletingBucket → Deleted
```

### Handler Actions

Handlers return:

```rust
enum HandlerAction {
    Continue { state: State, suggested_delay: Option<Duration> },
    Stay { suggested_delay: Option<Duration> },
}
```

- `Continue` - transition to next state
- `Stay` - remain in current state (for polling)

## The Executor

The `StackExecutor` orchestrates all controllers:

```rust
let executor = StackExecutor::new(&stack, client_config, lifecycle_filter)?;
let result = executor.step(current_state).await?;
```

Each `step()` call:

1. **Plans** - compares desired state to current state, identifies creates/updates/deletes
2. **Executes** - for each ready resource, calls its controller's handler
3. **Returns** - new state with updated resources

### Dependencies

Resources can depend on other resources:

```rust
let storage = Storage::new("data").build();
let function = Function::new("processor")
    .link(&storage)  // depends on storage
    .build();
```

The executor builds a dependency graph. A resource is "ready" when all its dependencies are in a terminal state (Running or Deleted).

During creation:
1. `storage` starts first
2. `function` waits until `storage` is Running
3. Once `storage` is Running, `function` starts

During deletion (reversed):
1. `function` deletes first
2. `storage` waits until `function` is Deleted

## Lifecycle Filters

Resources are marked `frozen` or `live`:

```typescript
export default new alien.Stack("my-stack")
  .add(dataStorage, "frozen")
  .add(fn, "live")
  .build()
```

The executor can filter by lifecycle:

```rust
// Only frozen resources
let executor = StackExecutor::new(&stack, config, Some(vec![ResourceLifecycle::Frozen]))?;

// Only live resources
let executor = StackExecutor::new(&stack, config, Some(vec![ResourceLifecycle::Live]))?;
```

This enables two-phase deployment:
1. **Initial setup** - deploy frozen resources (elevated permissions)
2. **Updates** - deploy live resources (minimal permissions)

## Platform Controllers

The same `Storage` resource has different controllers per platform:

| Platform | Controller | Backend |
|----------|------------|---------|
| AWS | `AwsStorageController` | S3 |
| GCP | `GcpStorageController` | Cloud Storage |
| Azure | `AzureStorageController` | Blob Storage |
| Local | `LocalStorageController` | Local filesystem |

The executor loads the correct controller based on `StackState.platform`.

## Error Handling

When a handler fails:

1. Executor increments `retry_attempt`
2. If retries < 5, retry with exponential backoff (2^attempt seconds)
3. If retries exhausted, transition to failure state

Non-retryable errors (invalid configuration) immediately fail.

## The Orchestration Loop

The caller drives the loop. In practice this runs inside whatever process manages the deployment — the CLI for initial setup, the Agent for ongoing operations in remote environments.

```rust
loop {
    let result = executor.step(current_state).await?;
    current_state = result.next_state;
    
    // Store state externally
    db.save(&deployment_id, &current_state).await?;
    
    if is_synced(&current_state) {
        break;
    }
    
    if let Some(delay_ms) = result.suggested_delay_ms {
        sleep(Duration::from_millis(delay_ms)).await;
    }
}
```

This loop runs in different contexts:

- **Lambda function** - state in DynamoDB, triggered by Step Functions
- **Temporal workflow** - state in workflow history
- **Terraform provider** - state in Terraform state file
- **Local CLI** — state in `~/.alien-cli/<deployment_id>/state.json`
- **Kubernetes Agent** — state in Custom Resource status

The library doesn't care where state lives or who calls `step()`.

## Implementation

### Crate Structure

```
alien-infra/
├── src/
│   ├── core/
│   │   ├── executor.rs     # StackExecutor
│   │   ├── controller.rs   # ResourceController trait
│   │   └── registry.rs     # ResourceRegistry
│   ├── function/
│   │   ├── aws.rs          # AwsFunctionController
│   │   ├── gcp.rs          # GcpFunctionController
│   │   └── local.rs        # LocalFunctionController
│   ├── storage/
│   │   ├── aws.rs          # AwsStorageController
│   │   └── ...
│   └── ...
```

### The Controller Macro

The `#[controller]` macro generates:
- State enum from handler attributes
- `ResourceController` trait implementation
- Serialization for state persistence
- State transition validation

### Binding Parameters

When resource A depends on resource B, A needs B's outputs (bucket name, queue URL, etc.).

Controllers expose this through `get_binding_params()`:

```rust
fn get_binding_params(&self) -> Option<serde_json::Value> {
    Some(json!({
        "bucketName": self.bucket_name,
        "region": self.region,
    }))
}
```

The executor makes these available to dependent resources.

