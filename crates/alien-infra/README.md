# alien-infra

Provisioning engine. Takes a desired state, compares it to current cloud state, and makes API calls to reconcile them. Each resource type has per-platform controllers (AWS, GCP, Azure, Local).

## Controller Pattern

Controllers are state machines. Each handler performs **one mutable operation**:

```rust
#[controller]
impl AwsMyResourceController {
    #[flow_entry(Create)]
    #[handler(state = CreateStart, on_failure = CreateFailed, status = ResourceStatus::Provisioning)]
    async fn create_start(&mut self, ctx: &ResourceControllerContext<'_>) -> Result<HandlerAction> {
        client.create_resource(&name).await?;
        self.resource_name = Some(name);
        Ok(HandlerAction::Continue { state: NextState, suggested_delay: None })
    }
}
```

Principles:
1. **One mutable operation per state** — enables proper retry on failure
2. **Linear flow** — always proceed through all states, even if no-op
3. **Best-effort deletion** — succeed even if resource is already gone
4. **Heartbeat in Ready** — verify resource exists on each heartbeat

## Structure

- `core/` — `StackExecutor` (orchestrates all controllers), `ResourceController` trait, `SingleControllerExecutor` (testing), controller registry
- `<resource>/` — Per-resource controllers with `aws.rs`, `gcp.rs`, `azure.rs`, `local.rs`, `templates.rs` (CloudFormation), `fixtures.rs`

## Testing

Use `SingleControllerExecutor` for controller tests. Every controller needs: create/delete tests, update tests, best-effort deletion tests, and validation tests.

## Change Detection for External State

The executor triggers updates by comparing resource configs via `resource_eq()`. Values that affect provisioning but live outside the config should be stamped onto the resource config in a preflight mutation. See `ContainerCluster.template_inputs` for the pattern.
