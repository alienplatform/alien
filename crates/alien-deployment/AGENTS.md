# alien-deployment

Deploy engine — a single resumable state machine that performs incremental deployment steps across all platforms.

## Core API

One public function: `step(current, config, client_config, service_provider) -> DeploymentStepResult`

Each call does ONE incremental step based on `current.status` and returns the complete next state. The caller (manager loop, CLI, controller) is responsible for:
- Acquiring a lock on the deployment before calling
- Saving the returned state
- Releasing the lock after updating

## Status Flow

```
Pending → InitialSetup → Provisioning → Running
Running → UpdatePending → Updating → Running
Running → DeletePending → Deleting → Deleted
*Failed states retry into their active counterpart*
```

## Key Files

- `lib.rs` — `step()` dispatcher, status-to-handler routing
- `pending.rs` — Applies preflight mutations, stores prepared stack, validates env vars
- `initial_setup.rs` — Deploys frozen resources (IAM, VPCs, vault) without env vars
- `provisioning.rs` — Deploys remaining resources with env var injection
- `running.rs` — Health checks (read-only, no config changes)
- `updating.rs` — Handles `UpdatePending`/`Updating` for stack changes
- `deleting.rs` — Handles `DeletePending`/`Deleting` for teardown
- `helpers.rs` — Shared utilities, `create_aggregated_error_from_stack_state`

## Mutation Strategy

- **Pending/UpdatePending**: Apply preflight mutations, store `prepared_stack` in runtime metadata
- **InitialSetup**: Use prepared stack, deploy frozen resources only
- **Provisioning/Updating**: Use prepared stack, inject env vars for functions/services
- **Running**: Use prepared stack for health checks (read-only)
- **Delete phases**: Use prepared stack for deletion (no env var injection)

## Don't

- Don't add platform-specific logic here — that belongs in `alien-infra` controllers
- Don't skip the state machine — always go through all states, even if no-op
- Don't call it "agent" — use "deployment"
