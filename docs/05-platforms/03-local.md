# Local Platform

The Local platform runs Alien applications as native processes on Windows, Linux, and macOS. Same controllers. Same bindings. Different primitives.

## Use Cases

**Local development** — Fast iterations without cloud credentials. `alien dev` starts your app locally.

**POCs and trials** — Enterprises can evaluate Alien applications without any cloud permissions. Download, run locally, see it work. No procurement, no security reviews, no IAM policies.

**Arbitrary VMs** — Almost any enterprise can provision a Linux VM with SSH access. Deploy an Alien deployment there. No Kubernetes, no container orchestration, no cloud-specific permissions. Works on EC2, VMware, bare metal, anything.

**Edge devices** — Robots, embedded systems, IoT devices running Linux.

## How It Works

Alien provides platform-agnostic resources: Storage, KV, Queue, Function. Each platform implements these differently:

| Resource | AWS | GCP | Local |
|----------|-----|-----|-------|
| Storage | S3 bucket | GCS bucket | Filesystem directory |
| KV | DynamoDB | Firestore | Sled embedded database |
| Function | Lambda | Cloud Run | Native process |
| Artifact Registry | ECR | Artifact Registry | In-process OCI server |

Your application code stays the same. You call `storage.put(key, data)`. The binding library handles the platform-specific implementation.

## Architecture

Cloud platforms have cloud services. S3 stores objects. Lambda runs functions. DynamoDB stores key-value data. Controllers call these services to provision resources.

Local platform has **local services** — implementations that provide the same Alien abstractions. `LocalStorageManager` provides Storage (backed by filesystem). `LocalFunctionManager` provides Function (backed by native processes). `LocalKvManager` provides KV (backed by Sled).

The pattern is the same. `AwsStorageController` calls S3 APIs. `LocalStorageController` calls `LocalStorageManager` methods. Both produce a Storage resource that user code accesses through the same `storage.put()` / `storage.get()` interface.

```
Cloud Platform                      Local Platform

┌──────────────────┐               ┌──────────────────┐
│    Controller    │               │    Controller    │
└────────┬─────────┘               └────────┬─────────┘
         │                                  │
         ▼                                  ▼
┌──────────────────┐               ┌──────────────────┐
│    Cloud API     │               │  Service Manager │
│ (S3, Lambda)     │               │ (local process)  │
└──────────────────┘               └──────────────────┘
```

Service managers provide the same abstraction as cloud APIs. Controllers don't know they're running locally.

## Entry Point

The `alien deploy` command (or `alien dev` for local development) starts the local platform:

```bash
alien deploy --platform local --token $TOKEN --name production
```

Here's what happens:

1. **Initialize services** — Creates `LocalServices` with all service managers
2. **Start bindings server** — gRPC server for resource access
3. **Run deployment loop** — Same `alien_deployment::step()` as cloud deployments
4. **Sync with platform** — Reports state to the Alien API

```
┌─────────────────────────────────────────────────────────────┐
│  alien deploy (local)                                        │
│                                                             │
│  ┌─────────────────┐    ┌────────────────────────────────┐ │
│  │ Bindings Server │◀───│ Function Processes              │ │
│  │ (gRPC :50051)   │    │ (inherit ALIEN_BINDINGS_GRPC)  │ │
│  └────────┬────────┘    └────────────────────────────────┘ │
│           │                                                 │
│           ▼                                                 │
│  ┌─────────────────────────────────────────────────────┐   │
│  │ LocalServices                                        │   │
│  │ ├── StorageManager                                   │   │
│  │ ├── KvManager                                        │   │
│  │ ├── VaultManager                                     │   │
│  │ ├── FunctionManager                                  │   │
│  │ └── ArtifactRegistryManager                          │   │
│  └─────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────┘
```

All state is scoped by deployment ID: `~/.alien-cli/<deployment_id>/`.

## Service Managers

Each resource type has a service manager in the `alien-local` crate.

### LocalStorageManager

Creates filesystem directories for Storage resources.

```rust
// Controller usage
let storage_mgr = ctx.service_provider.get_local_storage_manager()?;
let path = storage_mgr.create_storage(&config.id).await?;
```

Operations:
- `create_storage(id)` — Creates `{state_dir}/storage/{id}/`
- `delete_storage(id)` — Removes directory
- `get_binding(id)` — Returns `StorageBinding::local(path)`

Storage persists on disk. The manager is stateless.

### LocalKvManager

Creates Sled databases for KV resources.

```rust
let kv_mgr = ctx.service_provider.get_local_kv_manager()?;
let path = kv_mgr.create_kv(&config.id).await?;
```

Data persists in `{state_dir}/kv/{id}/`. Sled is an embedded key-value database.

### LocalVaultManager

Creates directories for Vault resources.

```rust
let vault_mgr = ctx.service_provider.get_local_vault_manager()?;
vault_mgr.create_vault(&config.id).await?;
```

Secrets persist in `{state_dir}/vault/{id}/`.

### LocalFunctionManager

Spawns and manages function processes.

```rust
let func_mgr = ctx.service_provider.get_local_function_manager()?;
let url = func_mgr.start_function(&config.id, env_vars).await?;
```

Operations:
- `start_function(id, env_vars)` — Extracts OCI image, spawns `alien_runtime::run`
- `stop_function(id)` — Graceful shutdown via channel
- `is_running(id)` — Checks task handle
- `get_binding(id)` — Returns `FunctionBinding::local(url)`

Functions run as tokio tasks. Each gets a unique port.

### LocalArtifactRegistryManager

Starts in-process OCI registry servers.

```rust
let registry_mgr = ctx.service_provider.get_local_artifact_registry_manager()?;
let url = registry_mgr.start_registry(&config.id).await?;
```

Uses the `container_registry` crate. Images persist in `{state_dir}/artifact_registry/{id}/`.

## Bindings

User code accesses resources through the bindings gRPC server:

```typescript
const ctx = await AlienContext.fromEnv();
const storage = await ctx.getBindings().loadStorage('my-storage');
await storage.put('key', Buffer.from('value'));
```

The flow:

1. `AlienContext.fromEnv()` reads `ALIEN_BINDINGS_GRPC_ADDRESS`
2. `loadStorage('my-storage')` sends gRPC request
3. `LocalBindingsProvider` queries `storage_manager.get_binding('my-storage')`
4. Returns `LocalStorage` client pointing to the directory
5. `storage.put()` writes to the filesystem

```
User Code                    gRPC Server                   Manager
   │                             │                            │
   │ storage.put("key", data)    │                            │
   │────────────────────────────>│                            │
   │                             │ get_binding("my-storage")  │
   │                             │───────────────────────────>│
   │                             │        StorageBinding      │
   │                             │<───────────────────────────│
   │                             │ [creates LocalStorage]     │
   │                             │ [writes to filesystem]     │
   │          Result             │                            │
   │<────────────────────────────│                            │
```

**Key insight**: Bindings query managers on every request. If a registry restarts on a new port, the next request gets the new URL automatically. No function restart needed.

## Function Lifecycle

The `LocalFunctionController` manages function state through handlers:

### ExtractingImage

Pulls the OCI image and extracts it to disk.

```rust
#[handler(state = ExtractingImage, ...)]
async fn extracting_image(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let func_mgr = ctx.service_provider.get_local_function_manager()?;
    let extracted_path = func_mgr.extract_image(&config.id, &code.image, registry_config).await?;
    self.extracted_image_path = Some(extracted_path);
    Ok(HandlerAction::Continue { state: StartingProcess, ... })
}
```

Images are lightweight — no OS layers. Just runtime binaries and application code.

### StartingProcess

Spawns the function runtime.

```rust
#[handler(state = StartingProcess, ...)]
async fn starting_process(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let func_mgr = ctx.service_provider.get_local_function_manager()?;
    let url = func_mgr.start_function(&config.id, env_vars).await?;
    self.function_url = Some(url);
    Ok(HandlerAction::Continue { state: Ready, ... })
}
```

The manager spawns `alien_runtime::run` in a tokio task. Functions inherit `ALIEN_BINDINGS_GRPC_ADDRESS` from the process environment.

### Ready

Periodic health checks.

```rust
#[handler(state = Ready, ...)]
async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let func_mgr = ctx.service_provider.get_local_function_manager()?;
    func_mgr.check_health(&config.id).await?;
    Ok(HandlerAction::Continue { state: Ready, suggested_delay: Some(Duration::from_secs(30)) })
}
```

## Auto-Recovery

Service managers automatically recover from crashes.

### Function Crashes

The `LocalFunctionManager` background task monitors functions:

```rust
async fn monitor_and_recover_loop(...) {
    // Recover from metadata on startup
    recover_all_functions().await;
    
    // Monitor every 5 seconds
    loop {
        for function in running_functions {
            if function.task_handle.is_finished() {
                // Crashed - restart from metadata
                restart_function_from_metadata(function.metadata).await;
            }
        }
        sleep(5s);
    }
}
```

Metadata persists in `{state_dir}/functions/{id}/metadata.json`. Recovery happens without platform connectivity.

### CLI Restarts

On startup, managers scan metadata directories and restart resources:

1. `LocalFunctionManager` reads `functions/*/metadata.json`
2. Restarts each function using saved configuration
3. Functions continue from where they left off

State directory structure:

```
~/.alien-cli/<deployment_id>/
├── state.json                      # Deployment state (synced to platform)
├── storage/
│   └── {resource_id}/              # Storage directories
├── kv/
│   └── {resource_id}/              # Sled databases
├── vault/
│   └── {resource_id}/              # Vault data
├── functions/
│   └── {function_id}/
│       ├── metadata.json           # Recovery metadata
│       └── ...                     # Extracted OCI image
└── artifact_registry/
    └── {registry_id}/
        ├── metadata.json           # Recovery metadata
        └── ...                     # Registry data
```

## State Management

**Platform is source of truth.** The deployment loop syncs state to the Alien API after every step.

```rust
loop {
    let update = alien_deployment::step(current, target_stack, config, ...).await?;
    
    // Sync immediately to platform
    let response = client.apply_deployment(&session_id, &update).await?;
    
    // Use platform's response (authoritative)
    if let Some(current_from_platform) = response.current {
        current = current_from_platform;
    }
    
    // Save to disk (cache for offline mode)
    save_state(&state_file, &current).await?;
}
```

If the platform is unreachable, the CLI continues with local state. Sync resumes when connectivity returns.

## Graceful Shutdown

`Ctrl+C` triggers graceful shutdown:

1. Signal handler broadcasts shutdown
2. Deployment loop stops
3. `LocalServices::shutdown()` waits for all background tasks
4. Functions receive shutdown signal and drain requests

```rust
impl LocalServices {
    pub async fn shutdown(mut self) {
        // Trigger shutdown for all background tasks
        let _ = self.shutdown_tx.send(());
        
        // Wait for all tasks to complete
        for task in self.background_tasks.drain(..) {
            task.await.ok();
        }
    }
}
```

## Differences from Cloud

| Aspect | Cloud | Local |
|--------|-------|-------|
| Resource provisioning | Cloud APIs | Service managers |
| Function execution | Container | Native process |
| Permissions | IAM roles | None (all resources accessible) |
| Auto-recovery | Cloud provider | Background tasks |
| Images | Full OS + runtime | Runtime + app only |
| Bindings source | Environment variables | gRPC to managers |

## Implementation

```
alien-local/                  # Service managers
├── src/
│   ├── lib.rs               # LocalServices aggregate
│   ├── storage_manager.rs
│   ├── kv_manager.rs
│   ├── vault_manager.rs
│   ├── function_manager.rs
│   ├── artifact_registry_manager.rs
│   └── local_bindings_provider.rs

alien-infra/                  # Controllers
├── src/
│   ├── function/local.rs    # LocalFunctionController
│   ├── storage/local.rs     # LocalStorageController
│   └── ...
```

Controllers live in `alien-infra`. Service managers live in `alien-local`. The split keeps controllers clean — they just call manager methods.

