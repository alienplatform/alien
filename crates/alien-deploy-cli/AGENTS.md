# alien-deploy-cli

Push-mode deployment CLI. Runs `alien-deploy up` / `alien-deploy down` with target-environment credentials from the developer's machine.

## Architecture

The CLI talks to alien-manager via the sync protocol (acquire → step loop → reconcile → release) but executes deployment steps locally using the developer's cloud credentials.

## Key Files

- `commands/up.rs` — `up_command` (full deploy), `push_initial_setup` (creation phases), `push_deletion` (deletion phases). Uses `alien-deployment`'s shared runner and loop contract.
- `commands/down.rs` — `down_command`. Requests deletion via manager API, then delegates to `push_deletion`.
- `commands/agent.rs` — Starts a pull-mode alien-agent container locally.
- `commands/list.rs` — Lists deployments from the manager.
- `commands/status.rs` — Shows deployment status.
- `deployment_tracking.rs` — Local file-based tracking of deployed instances (name → deployment ID, token, URL, platform).

## Push Deletion Flow

`push_deletion` drives `DeletePending → Deleting → Deleted` locally:

1. Acquires the deployment via sync/acquire
2. Transitions to `DeletePending` if needed
3. Runs the step loop with `LoopOperation::Delete` using the shared runner from `alien-deployment`
4. Reconciles and releases the lock, even on failure

This is also used by `alien-test` for E2E test teardown (`teardown_target` calls `push_deletion` directly).

## Don't

- Don't add deployment state machine logic — that belongs in `alien-deployment`
- Don't add manager-side logic — this is a client that talks to the manager API
- Don't call it "agent" — use "deployment"
