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
Pending → InitialSetup → Running
Running → UpdatePending → Updating → Running
Running → DeletePending → Deleting → Deleted
*Failed states retry into their active counterpart*
```

## Key Files

- `lib.rs` — `step()` dispatcher, status-to-handler routing
- `loop_contract.rs` — Canonical loop contract types (`LoopOperation`, `LoopOutcome`, `LoopStopReason`, `LoopResult`) and the `classify_status()` function that maps deployment statuses to loop outcomes. All callers must use this contract.
- `runner.rs` — Shared step-loop runner (`run_step_loop`). Calls `step()` repeatedly with a `RunnerPolicy` (max steps, operation, delay threshold). Transport-agnostic — callers manage locks and persistence.
- `pending.rs` — Applies preflight mutations, stores prepared stack, validates env vars
- `initial_setup.rs` — Deploys all resources (frozen first, then live with env var injection)
- `running.rs` — Health checks (read-only, no config changes)
- `updating.rs` — Handles `UpdatePending`/`Updating` for stack changes
- `deleting.rs` — Handles `DeletePending`/`Deleting` for teardown
- `helpers.rs` — Shared utilities, `create_aggregated_error_from_stack_state`

## Loop Contract

The loop contract (`loop_contract.rs`) defines how callers interpret deployment status after each step:

- **`classify_status(status, operation)`** — returns `Some(LoopResult)` when the status is terminal for the given operation, `None` when the loop should continue.
- **Deploy operation**: `Running` → Success, `Provisioning`/`Updating` → Handoff (neutral), `*Failed` → Failure
- **Delete operation**: `Deleted` → Success, `*Failed` → Failure
- Failed-but-synced statuses (e.g. `DeleteFailed` where `is_synced() == true`) always map to `Failure`, never `Success`.

The runner (`runner.rs`) wraps this contract into a step loop with a budget. Budget exceeded → `LoopOutcome::Failure`.

## Mutation Strategy

- **Pending/UpdatePending**: Apply preflight mutations, store `prepared_stack` in runtime metadata
- **InitialSetup**: Use prepared stack, deploy all resources (frozen first, then live with env var injection)
- **Running**: Use prepared stack for health checks (read-only)
- **Delete phases**: Use prepared stack for deletion (no env var injection)

## Don't

- Don't add platform-specific logic here — that belongs in `alien-infra` controllers
- Don't skip the state machine — always go through all states, even if no-op
- Don't call it "agent" — use "deployment"
- Don't duplicate loop contract tests in other crates — the canonical tests live here
