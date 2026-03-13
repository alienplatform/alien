# Adding a New Resource

This guide covers how to add a new resource type to Alien. Resources are the building blocks of stacks — Storage, Function, Queue, KV, etc.

Adding a resource touches several crates:

| Crate | What You Add |
|-------|--------------|
| `alien-core` | Resource definition, outputs, binding config |
| `alien-infra` | Controllers per platform (AWS, GCP, Azure, Local) |
| `alien-permissions` | Permission sets for the resource |
| `alien-bindings` | Trait, providers, gRPC service |
| `alien-test-server` | Integration test endpoint |
| `packages/core` | TypeScript wrapper (optional) |

This doc focuses on the hardest part: writing controllers. For a checklist of all files to touch, see `alien/docs/ADDING_NEW_RESOURCE.md`.

## Resource Definitions

Resources are defined in `alien-core/src/resources/<name>.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Builder)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[builder(start_fn = new)]
pub struct MyResource {
    #[builder(start_fn)]
    pub id: String,
    
    #[serde(default)]
    #[builder(default)]
    pub some_option: bool,
}

impl MyResource {
    pub const RESOURCE_TYPE: ResourceType = ResourceType::from_static("my-resource");
}

#[typetag::serde(name = "my-resource")]
impl ResourceDefinition for MyResource {
    fn resource_type() -> ResourceType { Self::RESOURCE_TYPE.clone() }
    fn id(&self) -> &str { &self.id }
    fn get_dependencies(&self) -> Vec<ResourceRef> { Vec::new() }
    // ... other trait methods
}
```

Key points:
- Use `bon::Builder` for the builder pattern
- Use `#[serde(rename_all = "camelCase")]` for JSON serialization
- Use `#[typetag::serde]` for polymorphic serialization
- Define `RESOURCE_TYPE` as a const

See `alien-core/src/resources/storage.rs` for a complete example.

## Controllers

Controllers are state machines that provision resources. Each platform (AWS, GCP, Azure, Local) has its own controller for each resource type.

### The Controller Macro

The `#[controller]` macro generates the state machine boilerplate:

```rust
#[controller]
pub struct AwsMyResourceController {
    // State fields — persisted between steps
    pub(crate) resource_name: Option<String>,
}

#[controller]
impl AwsMyResourceController {
    // Handlers go here
}
```

The macro generates:
- State enum from handler attributes
- `ResourceController` trait implementation
- Serialization for state persistence
- State transition validation

### State Flow Structure

Every controller has three flows:

```
Create: CreateStart → ... → Ready
Update: UpdateStart → ... → Ready  
Delete: DeleteStart → ... → Deleted
```

Plus terminal failure states: `CreateFailed`, `UpdateFailed`, `DeleteFailed`, `RefreshFailed`.

### Writing Handlers

Each handler performs **one mutable operation**:

```rust
#[flow_entry(Create)]
#[handler(
    state = CreateStart,
    on_failure = CreateFailed,
    status = ResourceStatus::Provisioning,
)]
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<MyResource>()?;
    let client = ctx.service_provider.get_aws_my_client(ctx.get_aws_config()?)?;
    
    // ONE mutable operation
    client.create_resource(&config.id).await?;
    
    // Store result in controller state
    self.resource_name = Some(format!("{}-{}", ctx.resource_prefix, config.id));
    
    Ok(HandlerAction::Continue {
        state: ConfiguringOptions,
        suggested_delay: None,
    })
}
```

Key attributes:
- `#[flow_entry(Create)]` — marks this as the entry point for the Create flow
- `#[flow_entry(Update, from = [Ready, RefreshFailed])]` — Update flow can start from Ready or RefreshFailed
- `#[flow_entry(Delete, from = [Ready, CreateFailed, UpdateFailed])]` — Delete can start from multiple states
- `state = X` — the state this handler runs in
- `on_failure = Y` — the state to transition to if this handler fails
- `status = Z` — the `ResourceStatus` while in this state

### Handler Actions

Handlers return one of two actions:

```rust
// Move to next state
Ok(HandlerAction::Continue {
    state: NextState,
    suggested_delay: Some(Duration::from_secs(2)),
})

// Stay in current state (for polling)
Ok(HandlerAction::Stay {
    max_times: 20,
    suggested_delay: Some(Duration::from_secs(5)),
})
```

Use `Stay` when waiting for an async operation (e.g., waiting for a Lambda to become Active).

### Terminal States

Declare terminal states with the macro:

```rust
terminal_state!(state = CreateFailed, status = ResourceStatus::ProvisionFailed);
terminal_state!(state = UpdateFailed, status = ResourceStatus::UpdateFailed);
terminal_state!(state = DeleteFailed, status = ResourceStatus::DeleteFailed);
terminal_state!(state = RefreshFailed, status = ResourceStatus::RefreshFailed);
terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
```

### Outputs and Bindings

Controllers expose two methods for external consumption:

```rust
fn build_outputs(&self) -> Option<ResourceOutputs> {
    self.resource_name.as_ref().map(|name| {
        ResourceOutputs::new(MyResourceOutputs {
            resource_name: name.clone(),
        })
    })
}

fn get_binding_params(&self) -> Option<serde_json::Value> {
    if let Some(name) = &self.resource_name {
        let binding = MyResourceBinding::aws(name.clone());
        serde_json::to_value(binding).ok()
    } else {
        None
    }
}
```

- `build_outputs()` — exposed to users via the API
- `get_binding_params()` — used internally for injecting bindings into dependent resources

## Controller Design Principles

### One Mutable Operation Per State

**Why?** If a handler fails, the executor retries from that state. If you do multiple operations in one handler, you can't retry just the failed one.

**Bad:**
```rust
async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    client.create_bucket(&name).await?;
    client.put_versioning(&name).await?;  // If this fails, bucket is already created
    client.put_lifecycle(&name).await?;   // Can't retry just this part
    Ok(...)
}
```

**Good:**
```rust
async fn create_start(&mut self, ...) -> Result<HandlerAction> {
    client.create_bucket(&name).await?;
    Ok(HandlerAction::Continue { state: ConfiguringVersioning, ... })
}

async fn configuring_versioning(&mut self, ...) -> Result<HandlerAction> {
    client.put_versioning(&name).await?;
    Ok(HandlerAction::Continue { state: ConfiguringLifecycle, ... })
}
```

### Linear Flow Principle

Always proceed through all states in order, even if some states have no work to do:

```rust
async fn configuring_versioning(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<Storage>()?;
    
    if config.versioning {
        client.put_versioning(&self.bucket_name, true).await?;
    }
    // Always continue to next state, even if we didn't do anything
    
    Ok(HandlerAction::Continue {
        state: ConfiguringLifecycle,
        suggested_delay: None,
    })
}
```

This makes the state machine predictable and easier to debug.

### Best-Effort Deletion

Deletion should succeed even if the resource is already gone:

```rust
async fn delete_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    // Handle case where resource was never created
    let name = match &self.resource_name {
        Some(n) => n,
        None => {
            return Ok(HandlerAction::Continue { state: Deleted, ... });
        }
    };
    
    // Best effort - ignore NotFound errors
    match client.delete_resource(name).await {
        Ok(_) => info!("Resource deleted"),
        Err(e) if matches!(e.error, Some(CloudClientErrorData::RemoteResourceNotFound { .. })) => {
            warn!("Resource already deleted or never existed");
        }
        Err(e) => {
            // Log but continue - don't fail deletion
            warn!(?e, "Could not delete resource, continuing anyway");
        }
    }
    
    self.resource_name = None;
    Ok(HandlerAction::Continue { state: Deleted, ... })
}
```

### Ready State Heartbeat

The Ready state should verify the resource still exists:

```rust
#[handler(state = Ready, on_failure = RefreshFailed, status = ResourceStatus::Running)]
async fn ready(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let client = ctx.service_provider.get_aws_my_client(ctx.get_aws_config()?)?;
    
    // Verify resource exists
    client.head_resource(&self.resource_name).await
        .context(ErrorData::CloudPlatformError {
            message: "Failed to verify resource during heartbeat".to_string(),
            resource_id: Some(config.id.clone()),
        })?;
    
    debug!(name = %config.id, "Heartbeat check passed");
    
    Ok(HandlerAction::Continue {
        state: Ready,
        suggested_delay: Some(Duration::from_secs(30)),
    })
}
```

### Update Flow

For updates, compare current and previous config:

```rust
#[flow_entry(Update, from = [Ready, RefreshFailed])]
#[handler(state = UpdateStart, on_failure = UpdateFailed, status = ResourceStatus::Updating)]
async fn update_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
    let config = ctx.desired_resource_config::<MyResource>()?;
    let prev_config = ctx.previous_resource_config::<MyResource>()?;
    
    if config.some_option != prev_config.some_option {
        client.update_option(&self.resource_name, config.some_option).await?;
    }
    
    Ok(HandlerAction::Continue { state: Ready, ... })
}
```

### Accessing Dependencies

When your resource depends on another resource:

```rust
let dependency_ref = ResourceRef::new(Storage::RESOURCE_TYPE, "my-storage".to_string());
let storage_controller = ctx.require_dependency::<AwsStorageController>(&dependency_ref)?;

// Now you can access the storage controller's state
let bucket_name = storage_controller.bucket_name
    .ok_or_else(|| AlienError::new(ErrorData::DependencyNotReady { ... }))?;
```

## Testing Controllers

### The SingleControllerExecutor

Use `SingleControllerExecutor` for unit testing controllers:

```rust
#[tokio::test]
async fn test_create_and_delete_flow() {
    let resource = MyResource::new("test-resource").build();
    let mock_client = setup_mock_client();
    let mock_provider = setup_mock_service_provider(mock_client);
    
    let mut executor = SingleControllerExecutor::builder()
        .resource(resource)
        .controller(AwsMyResourceController::default())
        .platform(Platform::Aws)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    
    // Run create flow
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
    
    // Verify outputs
    let outputs = executor.outputs().unwrap();
    let my_outputs = outputs.downcast_ref::<MyResourceOutputs>().unwrap();
    assert!(my_outputs.resource_name.starts_with("test-"));
    
    // Delete
    executor.delete().unwrap();
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);
}
```

### Test Categories

Every controller should have these test categories:

**1. Create and Delete Flow Tests**

Test that each resource configuration variant can be created and deleted:

```rust
#[rstest]
#[case::basic(basic_resource())]
#[case::with_option_a(resource_with_option_a())]
#[case::with_option_b(resource_with_option_b())]
#[case::all_options(resource_with_all_options())]
#[tokio::test]
async fn test_create_and_delete_flow_succeeds(#[case] resource: MyResource) {
    // Setup and run create/delete flow
}
```

**2. Update Flow Tests**

Test transitions between different configurations:

```rust
#[rstest]
#[case::basic_to_option_a(basic_resource(), resource_with_option_a())]
#[case::option_a_to_option_b(resource_with_option_a(), resource_with_option_b())]
#[case::all_to_basic(resource_with_all_options(), basic_resource())]
#[tokio::test]
async fn test_update_flow_succeeds(#[case] from: MyResource, #[case] to: MyResource) {
    // Ensure same ID
    let mut from = from;
    let mut to = to;
    from.id = "test-update".to_string();
    to.id = "test-update".to_string();
    
    // Start in Ready state with mock_ready controller
    let ready_controller = AwsMyResourceController::mock_ready("test-update");
    
    // Build executor, update, verify
}
```

**3. Best-Effort Deletion Tests**

Test that deletion succeeds even when resources are missing:

```rust
#[tokio::test]
async fn test_best_effort_deletion_when_resource_missing() {
    // Setup mock that returns NotFound
    // Verify deletion still succeeds
}
```

**4. Validation Tests**

Test that specific configurations generate correct API calls:

```rust
#[tokio::test]
async fn test_option_generates_correct_api_call() {
    let resource = resource_with_option_a();
    
    let mut mock_client = MockClient::new();
    mock_client
        .expect_some_method()
        .withf(|arg| {
            // Validate the argument
            arg.option_a == true
        })
        .returning(|_| Ok(()));
    
    // Run and verify
}
```

### Test Fixtures

Create fixtures for common resource configurations:

```rust
#[fixture]
fn basic_resource() -> MyResource {
    MyResource::new("basic".to_string()).build()
}

#[fixture]
fn resource_with_option_a() -> MyResource {
    MyResource::new("option-a".to_string())
        .option_a(true)
        .build()
}
```

### Mock Ready Controllers

Provide a `mock_ready` constructor for update tests:

```rust
impl AwsMyResourceController {
    #[cfg(feature = "test-utils")]
    pub fn mock_ready(name: &str) -> Self {
        Self {
            state: AwsMyResourceState::Ready,
            resource_name: Some(format!("test-stack-{}", name)),
            _internal_stay_count: None,
        }
    }
}
```

## Registration

Register controllers in `alien-infra/src/core/registry.rs`:

```rust
#[cfg(feature = "aws")]
registry.register_controller_factory(
    MyResource::RESOURCE_TYPE,
    Platform::Aws,
    Box::new(DefaultControllerFactory::<
        crate::my_resource::AwsMyResourceController,
    >::new()),
);
```

## Bindings

If your resource needs to be accessible from applications at runtime:

1. Define binding config in `alien-core/src/bindings/<name>.rs`
2. Implement provider in `alien-bindings/src/providers/<name>/`
3. Add gRPC service in `alien-bindings/src/grpc/<name>_service.rs`
4. Add proto definition in `alien-bindings/proto/<name>.proto`

See `alien-core/src/bindings/storage.rs` and `alien-bindings/src/providers/storage/` for examples.

## Permissions

Add permission sets in `alien-permissions/permission-sets/<name>/`:

```jsonc
// data-read.jsonc
{
  "id": "my-resource/data-read",
  "description": "Allows reading from my-resource",
  "platforms": {
    "aws": [{
      "grant": {
        "actions": ["myservice:GetItem", "myservice:ListItems"]
      },
      "binding": {
        "stack": {
          "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:resource/${stackPrefix}-*"]
        },
        "resource": {
          "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:resource/${resourceName}"]
        }
      }
    }]
  }
}
```

## Integration Testing

Add a test endpoint in `alien-test-server/src/handlers/<name>.rs`:

```rust
pub async fn test_my_resource(
    State(app_state): State<AppState>,
    Path(binding_name): Path<String>,
) -> Result<Json<MyResourceTestResponse>> {
    let instance = app_state.ctx.get_bindings()
        .load_my_resource(&binding_name).await?;
    
    // Test basic operations
    instance.create_item("test-key", "test-value").await?;
    let value = instance.get_item("test-key").await?;
    instance.delete_item("test-key").await?;
    
    Ok(Json(MyResourceTestResponse { success: true }))
}
```

## Checklist

Use this checklist when adding a new resource. Not all steps apply to every resource.

### Core Definition
- [ ] `alien-core/src/resources/<name>.rs` — resource struct with `#[derive(Builder)]`
- [ ] `alien-core/src/resources/mod.rs` — export the resource
- [ ] `alien-core/src/bindings/<name>.rs` — binding enum if runtime access needed
- [ ] `alien-core/src/bindings/mod.rs` — export the binding

### Cloud Clients
- [ ] `alien-cloud-clients/src/aws/<service>.rs` — AWS API client
- [ ] `alien-cloud-clients/src/gcp/<service>.rs` — GCP API client  
- [ ] `alien-cloud-clients/src/azure/<service>.rs` — Azure API client

See the `AGENTS.md` in each platform directory for client patterns.

### Controllers
- [ ] `alien-infra/src/<name>/aws.rs` — AWS controller
- [ ] `alien-infra/src/<name>/gcp.rs` — GCP controller
- [ ] `alien-infra/src/<name>/azure.rs` — Azure controller
- [ ] `alien-infra/src/<name>/local.rs` — Local controller (if applicable)
- [ ] `alien-infra/src/<name>/mod.rs` — module exports
- [ ] `alien-infra/src/core/registry.rs` — register controllers

### CloudFormation (AWS)
- [ ] `alien-infra/src/<name>/templates.rs` — CFN generator and importer
- [ ] `alien-infra/src/core/registry.rs` — register CloudFormation importer

### GCP API Enablement
If your resource requires enabling a Google Cloud API:
- [ ] `alien-build/src/infra_requirements.rs` — add `GCPProjectService` for the API

### Permissions
- [ ] `alien-permissions/permission-sets/<name>/data-read.jsonc`
- [ ] `alien-permissions/permission-sets/<name>/data-write.jsonc`
- [ ] `alien-permissions/permission-sets/<name>/management.jsonc`
- [ ] `alien-permissions/permission-sets/<name>/provision.jsonc`
- [ ] Run `cargo test -p alien-permissions` to verify

### Bindings (if runtime access needed)
- [ ] `alien-bindings/src/traits.rs` — add trait
- [ ] `alien-bindings/src/providers/<name>/aws.rs` — AWS provider
- [ ] `alien-bindings/src/providers/<name>/gcp.rs` — GCP provider
- [ ] `alien-bindings/src/providers/<name>/azure.rs` — Azure provider
- [ ] `alien-bindings/src/providers/<name>/local.rs` — Local provider
- [ ] `alien-bindings/src/providers/<name>/grpc.rs` — gRPC client provider
- [ ] `alien-bindings/src/providers/<name>/mod.rs` — module exports
- [ ] `alien-bindings/src/provider.rs` — add loader

### gRPC Service
- [ ] `alien-bindings/proto/<name>.proto` — protocol definition
- [ ] `alien-bindings/src/grpc/<name>_service.rs` — service implementation
- [ ] `alien-bindings/src/grpc/mod.rs` — export service
- [ ] `alien-bindings/src/grpc/server.rs` — register service
- [ ] `alien-bindings/build.rs` — add proto to build

### Integration Testing
- [ ] `alien-test-server/src/handlers/<name>.rs` — test endpoint
- [ ] `alien-test-server/src/handlers/mod.rs` — export handler
- [ ] `alien-test-server/src/lib.rs` — register route
- [ ] `alien-test-server/src/models.rs` — response types

### TypeScript SDK
- [ ] Run `pnpm -w --filter @alien/core run generate` — generates schemas
- [ ] `packages/core/src/<name>.ts` — wrapper (optional)
- [ ] `packages/core/src/index.ts` — export wrapper
- [ ] Run `pnpm -w --filter @alien/core run test:ts` — verify types

## Summary

The best way to learn is to study existing resources. Start with Storage (simpler) before looking at Function (more complex).

