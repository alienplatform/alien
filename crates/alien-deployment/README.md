# alien-deployment

Deployment state machine — a single resumable engine that performs incremental deployment steps across all platforms.

## Core API

One public function: `step(current, config, client_config, service_provider) -> DeploymentStepResult`

Each call does ONE incremental step based on `current.status` and returns the complete next state. The caller (manager loop, deploy CLI, agent) acquires a lock before calling, saves the returned state, and releases the lock after.

## Status Flow

```
Pending → InitialSetup → Provisioning → Running
Running → UpdatePending → Updating → Running
Running → DeletePending → Deleting → Deleted
```

Each active status has a corresponding failed state (`InitialSetupFailed`, `ProvisioningFailed`, `UpdateFailed`, `DeleteFailed`, `RefreshFailed`) that retries into its active counterpart.

## Loop Contract

`loop_contract.rs` defines how callers interpret deployment status after each step. Three operations:

- **`Deploy`** — Full deploy. `Running` = Success, `*Failed` = Failure. Continues through `Provisioning`/`Updating` (not terminal).
- **`InitialSetup`** — Push-mode initial setup only. Stops at `Provisioning`/`Updating` with `Handoff` so the manager takes over. Used by `alien-deploy-cli`.
- **`Delete`** — Full teardown. `Deleted` = Success, `*Failed` = Failure.

The shared runner (`runner.rs`) wraps this contract into a step loop with a budget.

## Mutation Strategy

- **Pending/UpdatePending** — Apply preflight mutations, store `prepared_stack` in runtime metadata
- **InitialSetup** — Deploy all resources (frozen first, then live with env var injection)
- **Running** — Health checks (read-only)
- **Delete phases** — Teardown using prepared stack
