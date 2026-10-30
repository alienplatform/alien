# alien-infra

Provisioning engine. Takes a desired state, compares it to current cloud state, and makes API calls to reconcile them. Each resource type has per-platform controllers (AWS, GCP, Azure, Local).

## Controller Pattern

Controllers are state machines. Each handler is a retry boundary. Prefer one
externally durable mutating operation per handler:

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
1. **Retry-safe mutation boundary** — one durable remote mutation per handler by default
2. **Stable phase order** — proceed through predictable phases, even if some are no-ops
3. **Best-effort deletion** — succeed even if resource is already gone
4. **Heartbeat in Ready** — verify resource exists on each heartbeat

Composite controllers such as networks, exposed containers, and compute
clusters may batch child operations only when the phase is idempotent,
progress-tracked in controller state, or best-effort cleanup.

## Structure

- `core/` — `StackExecutor` (orchestrates all controllers), `ResourceController` trait, `SingleControllerExecutor` (testing), controller registry
- `<resource>/` — Per-resource controllers with `aws.rs`, `gcp.rs`, `azure.rs`, `local.rs`, `templates.rs` (CloudFormation), `fixtures.rs`

## Testing

Use `SingleControllerExecutor` for controller tests. Every controller needs: create/delete tests, update tests, best-effort deletion tests, and validation tests.

## Change Detection for External State

The executor triggers updates by comparing resource configs via `resource_eq()`. Values owned by management after setup should live in management config or controller state, not in Frozen resource config.

## AWS service-account teardown

With automatic setup, runtime cleanup removes service-account roles created by
the controller after their dependent workloads have stopped. A retained data
resource's permission grant does not keep its grantee role alive by itself;
actual resource dependencies still control deletion order. Setup-owned resources
remain for package teardown.

The controller records role creation in `runtimeCreated`. For older checkpoints
without that marker, it reads the role and requires all three creation tags:
`managed-by=runtime`, `deployment=<resource prefix>`, and `resource=<resource ID>`.
A matching role name alone is insufficient. Roles without this ownership proof
are preserved with their identifiers, and runtime cleanup completes. Older
untagged roles can therefore remain after cleanup; this compatibility path does
not adopt or retag them. Explicit privileged setup teardown is unchanged.

Cleanup detaches managed policies and deletes inline policies before deleting
the role. Unexpected IAM errors retain the role identifiers and use the
executor's normal failure and retry path. When a failed cleanup is resumed from
an older checkpoint, the controller checks live ownership again before further
IAM writes. Already-missing roles or policies do not block cleanup.

Run the executor deletion tests and service-account controller tests to verify
workload ordering, setup-owned role preservation, older checkpoints, and retries
after partial policy cleanup:

```sh
cargo test -p alien-infra --all-features --lib core::executor_tests::deletion_tests
cargo test -p alien-infra --all-features --lib service_account::aws::tests
```

For a live AWS check, install an automatic-setup Terraform package with a worker
and a Live storage resource. Keep the original restricted installer session
available for both apply and destroy. Verify the worker can use its storage and
record the execution role's ownership tags before requesting runtime erase.
After runtime cleanup completes, require `GetRole` to return `NoSuchEntity` for
the execution role **before** running `terraform destroy`. Then require destroy
to succeed in the original installer session, Terraform state to be empty, and
an independent inventory to find no resources with that deployment prefix.
Successful destroy or empty Terraform state alone does not prove runtime cleanup:
Terraform does not track roles created by the runtime controller. Record any
privileged repair separately; a repaired run does not pass this check.

## Azure storage lifecycle in direct setup

The shared storage-account controller writes one aggregate policy during explicit
**direct setup**. Imported Terraform accounts remain setup-owned; runtime
container controllers only read the shared settings. Use one setup owner per
account and serialize setup runs: Azure replaces the entire policy on PUT.

Rerunning direct setup also reads Azure when declarations have not changed. It
repairs drift through the normal saved update phases, preserves rule ownership,
and fails on denied reads before attempting changes. A matching saved declaration
revision alone does not establish that cloud settings are still correct.

Direct setup creates new or changed expiry rules **disabled**, for both Live and
Frozen containers. Older installations ignored some expiry declarations, so an
upgrade must not suddenly expire existing objects. After setup finishes, open
**Storage account → Lifecycle management** in Azure, review each generated
`alien…` rule's exact container/prefix and age, and explicitly enable the rules
whose existing data may expire. Unchanged rules preserve this choice on the next
setup run. Changing the prefix or age creates a new disabled rule and disables
the previous owned rule. Terraform installations instead use their generated
lifecycle enable variables; do not run direct setup against that policy.

Removing a declaration disables its owned rule. Disabled obsolete rules remain
in the policy, including when the last declaration is removed; unrelated rules
are preserved. Once Azure's propagation period has elapsed, an installer can
remove those obsolete disabled rules in Azure. Setup refuses policies exceeding
100 total rules or same-named rules without its saved ownership receipt; keep the
original deployment checkpoints for recovery. Setup never adopts a rule merely
because its name matches.

A successful policy readback proves configuration, not that background expiry
has stopped. Azure documents up to 24 hours for a policy change to take effect.
Do not claim retained data is protected during that propagation window. See
[Azure lifecycle policy behavior](https://learn.microsoft.com/en-us/azure/storage/blobs/lifecycle-management-policy-faq).

Controller readers must support `ConfiguringLifecycle`, `VerifyingLifecycle`,
`UpdatingLifecycle`, and `VerifyingUpdatedLifecycle` before setup writes these
checkpoints. Old checkpoints default `lifecycleRuleNames` to empty and retain
account identity; imported accounts have no direct-setup rule ownership.
