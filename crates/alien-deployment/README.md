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
- **InitialSetup** — Deploy setup-owned Frozen resources.
- **Provisioning/Updating** — Deploy Alien-owned Live resources with env var injection.
- **Running** — Health checks (read-only)
- **Delete phases** — Teardown using prepared stack

## Cleanup and permission changes

A denied or unknown delete is a failure. The engine retains the resource's controller
checkpoint, identifiers, and error so an explicit retry can continue after access is
restored. A provider controller may treat an individual child as absent when that
operation returns NotFound; it must still clean up the remaining children. An AWS
confirmation read denied after a recorded successful delete is different from a
denied delete followed by a denied read.

Removing resources or owned secret values runs under the installed permissions.
An ordinary update that finishes that cleanup but still needs setup-owned IAM
changes reports a setup-required failure. It retains both the installed prepared
stack and the desired pending stack until privileged setup reconciles the grants.
Automatic setup handles a narrowing-only update by updating retained consumers and
cleaning up their old dependencies first, then narrowing permissions in InitialSetup.
Runtime roles do not need broad IAM policy deletion permissions.

For manual, Terraform, or CloudFormation setup, apply the runtime cleanup update
before applying the narrower setup definition. Changing external IAM first can
remove the authority needed to finish cleanup; restore the necessary installed
permissions and retry if that has happened. Registration after a template apply is
not an atomic barrier against premature external permission changes.

A release that needs new setup resources or grants while retained consumers still
reference resources being removed requires an intermediate release. First detach
those consumers and remove the old resources using the installed permissions; then
apply setup for the new resources and grants. Setup refuses the blocked mixed change
before changing IAM instead of waiting indefinitely or deleting a live dependency.

### Previously lost deletion checkpoints

Setup checkpoints written by older executors are normalized on load using a
deployment protocol version. Their prepared desired stack must not be
overwritten by a stale pending stack from a failed runtime update. Version 3 setup checkpoints are rejected by version 1 and 2 executors before provider work. Version 2 remains reserved for GCP federation. Manual code-only updates retain their existing protocol. Updates that defer setup-owned definitions or permission cleanup, and direct setup updates, use version 3. Upgrade all
executors that can read a deployment before allowing them to write the new
installed/pending setup representation. Once a checkpoint is upgraded to version 3, every executor that reads it must
support version 3, including after setup finishes and the deployment is Running.
Finishing setup does not downgrade the checkpoint; older executors cannot resume it.

Upgrading cannot recover identifiers that an older executor discarded when it
reported Deleted. Before repairing such a deployment, an operator must identify the
exact owned objects using independent inventory and ownership evidence. Explicitly
choose to remove those objects with appropriate authority or restore a verified
checkpoint for retry. Do not reconstruct ownership from a matching name alone or
adopt an ambiguous existing object. This recovery is separate from normal retries.
