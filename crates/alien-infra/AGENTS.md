# alien-infra

This crate provisions infrastructure resources. It takes a desired state, compares it to current state, and makes API calls to reconcile them.

Use `--all-features` when running `cargo check` or `cargo build`. 

## Quick Links

- **Adding a new resource?** See `docs/10-guides/00-adding-resources.md`

## Crate Structure

```
alien-infra/
├── src/
│   ├── core/
│   │   ├── executor.rs        # StackExecutor - orchestrates all controllers
│   │   ├── controller.rs      # ResourceController trait and context
│   │   ├── controller_test.rs # SingleControllerExecutor for testing
│   │   ├── registry.rs        # Maps (resource_type, platform) → controller
│   │   └── environment_variables.rs
│   ├── <resource>/
│   │   ├── aws.rs             # AwsResourceController
│   │   ├── gcp.rs             # GcpResourceController
│   │   ├── azure.rs           # AzureResourceController
│   │   ├── local.rs           # LocalResourceController (if applicable)
│   │   ├── templates.rs       # CloudFormation generator/importer (AWS)
│   │   └── fixtures.rs        # Test fixtures
│   └── ...
```

## Controller Pattern

Controllers are state machines. Each handler performs **one mutable operation**:

```rust
#[controller]
pub struct AwsMyResourceController {
    pub(crate) resource_name: Option<String>,
}

#[controller]
impl AwsMyResourceController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        // One API call
        client.create_resource(&name).await?;
        self.resource_name = Some(name);
        
        Ok(HandlerAction::Continue { state: NextState, suggested_delay: None })
    }
    
    terminal_state!(state = CreateFailed, status = ResourceStatus::ProvisionFailed);
    terminal_state!(state = Deleted, status = ResourceStatus::Deleted);
    // ...
}
```

### Key Principles

1. **One mutable operation per state** — enables proper retry on failure
2. **Linear flow** — always proceed through all states, even if no-op
3. **Best-effort deletion** — deletion should succeed even if resource is gone
4. **Heartbeat in Ready** — verify resource exists on each heartbeat

## Testing

Use `SingleControllerExecutor` for controller tests:

```rust
#[tokio::test]
async fn test_create_and_delete() {
    let mut executor = SingleControllerExecutor::builder()
        .resource(my_resource)
        .controller(AwsMyResourceController::default())
        .platform(Platform::Aws)
        .service_provider(mock_provider)
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
    
    executor.delete().unwrap();
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Deleted);
}
```

### Required Test Categories

Every controller needs:

1. **Create/Delete tests** — all configuration variants
2. **Update tests** — transitions between configurations  
3. **Best-effort deletion** — when resources already gone
4. **Validation tests** — verify correct API calls

See existing controllers (e.g., `storage/aws.rs`) for test examples.

## Adding a New Resource

See `docs/10-guides/00-adding-resources.md` for the complete guide with checklist.

## Common Patterns

### Accessing Context

```rust
let config = ctx.desired_resource_config::<MyResource>()?;
let prev_config = ctx.previous_resource_config::<MyResource>()?;
let aws_cfg = ctx.get_aws_config()?;
let client = ctx.service_provider.get_aws_my_client(aws_cfg)?;
```

### Dependencies

```rust
let ref = ResourceRef::new(Storage::RESOURCE_TYPE, "my-storage".to_string());
let storage = ctx.require_dependency::<AwsStorageController>(&ref)?;
let bucket_name = storage.bucket_name.ok_or_else(|| ...)?;
```

### Error Handling

```rust
client.do_something().await
    .context(ErrorData::CloudPlatformError {
        message: "Failed to do something".to_string(),
        resource_id: Some(config.id.clone()),
    })?;
```

### Polling with Stay

```rust
Ok(HandlerAction::Stay {
    max_times: 20,
    suggested_delay: Some(Duration::from_secs(5)),
})
```

### Change Detection for External State

The executor triggers updates by comparing resource configs via `resource_eq()`. If a value affects provisioning but lives outside the config (e.g., in `DeploymentConfig`), changes to it won't be detected.

**Solution:** Stamp external values onto the resource config in a preflight mutation. This makes them part of the config comparison — no executor changes needed. See `ContainerCluster.template_inputs` for the pattern: a preflight mutation reads from `DeploymentConfig` and writes onto the resource config before the executor sees it.

Use `#[builder(skip)]` on these system-populated fields so users can't set them via the builder:

```rust
#[builder(skip)]
#[serde(skip_serializing_if = "Option::is_none")]
pub template_inputs: Option<TemplateInputs>,
```

### Sensitive Values in Configs

Never store raw secrets (auth tokens, API keys) in resource configs — they're serialized to state and may be visible. Store a **hash** for change detection and read the actual value from `DeploymentConfig` at provisioning time:

```rust
pub struct TemplateInputs {
    pub horizond_download_base_url: String,       // non-sensitive: store directly
    pub monitoring_logs_endpoint: Option<String>,  // non-sensitive: store directly
    pub monitoring_auth_hash: Option<String>,      // SHA-256 hash only — actual value from ctx.deployment_config
}
```

