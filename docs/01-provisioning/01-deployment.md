# Deployment

The `alien-deployment` crate orchestrates the full deployment lifecycle.

## What It Does

If `alien-infra` is the engine that creates individual cloud resources, `alien-deployment` is the gearbox that sequences the whole process — preflights first, frozen resources next, then live resources, then ongoing health checks.

`alien-infra` provisions cloud resources. But a deployment needs more:

- **Preflights** — Validate stack, apply mutations (add service accounts, vault, network)
- **Secrets** — Sync secret env vars to vault before functions start
- **Env vars** — Inject plain env vars into function configs
- **Compatibility** — Check if updates would break frozen resources
- **Health** — Verify resources exist and work correctly

`alien-deployment` coordinates all of this on top of `alien-infra`.

One `alien-infra` step = one controller action (create bucket, configure versioning).  
One `alien-deployment` step = run preflights OR call alien-infra OR sync secrets OR check health.

The deployment lifecycle tracks which phase you're in (initial-setup, provisioning, running, updating). alien-infra doesn't know about phases — it just provisions resources.

## The Interface

```rust
let result = alien_deployment::step(current, config, client_config).await?;
```

Call `step()` with the current state. It does one thing based on the status. Returns the complete next state plus hints for the caller.

```rust
pub struct DeploymentStepResult {
    pub state: DeploymentState,           // Complete next state
    pub suggested_delay_ms: Option<u64>,  // Wait hint
    pub update_heartbeat: bool,            // Monitoring signal
}
```

The caller saves the full state directly. No delta merge logic needed.

## Walking Through a Deployment

### Pending

Every deployment starts here.

`step()` does three things:

1. **Collects environment info** - AWS account ID, GCP project number, region. Stored for later.

2. **Runs preflights** - Validates the stack. Applies mutations that add infrastructure (service accounts, vault). The result is the "prepared stack."

3. **Stores the prepared stack** - All subsequent phases use this, not the original user stack.

Transitions to `InitialSetup`.

### InitialSetup

Deploys frozen resources using elevated credentials.

Each `step()` calls `alien-infra` with a filter: only `Frozen` lifecycle resources. Multiple calls may be needed - `alien-infra` does one thing per step too.

When all frozen resources are running, transitions to `Provisioning`.

### Provisioning

Deploys live resources.

Before calling `alien-infra`, each `step()`:
1. Syncs secrets to vault (skips if already synced, tracked by hash)
2. Injects plain environment variables into function configs

Then calls `alien-infra` with filter: only `Live` lifecycle resources.

When all live resources are running, transitions to `Running`.

### Running

Deployment complete. But `step()` still does something: health checks.

Each `step()` verifies resources exist and are configured correctly. If something's wrong, transitions to `RefreshFailed`.

## Updates

```
Running → UpdatePending → Updating → Running
```

**UpdatePending** runs preflights with compatibility checks - compares old and new prepared stacks for breaking changes.

**Updating** works like Provisioning. Frozen resources are never updated.

## Failures

Each phase has a failed state: `InitialSetupFailed`, `ProvisioningFailed`, `RefreshFailed`, etc.

On failure, the deployment pauses. When `retry_requested` is set, `step()` retries failed resources and returns to the active state.

## What's Stored

```rust
pub struct DeploymentState {
    pub status: DeploymentStatus,          // Where we are
    pub platform: Platform,                // Target platform
    pub current_release: Option<ReleaseInfo>, // Currently deployed
    pub target_release: Option<ReleaseInfo>,  // Target to deploy
    pub stack_state: Option<StackState>,   // Resource states from alien-infra (nullable)
    pub environment_info: Option<...>,     // Cloud account details
    pub runtime_metadata: Option<...>,     // Prepared stack, sync tracking
    pub retry_requested: bool,             // Retry failed resources
}
```

The caller persists this state between `step()` calls. `StackState` is null until the first deployment begins.

## Configuration Flow

The caller builds `DeploymentConfig` and passes it to alien-deployment. It contains immutable configuration provided by the user (`stack_settings`) and management configuration derived from the environment.

```rust
pub struct DeploymentConfig {
    pub stack_settings: StackSettings,        // User's network/deployment model/approval choices
    pub management_config: Option<ManagementConfig>,  // Cross-account credentials (push) or null (pull)
    pub environment_variables: EnvironmentVariablesSnapshot,
    // ...
}
```

### Key Insight

alien-deployment works identically regardless of who calls it. The differences:
- Who calls it (CLI, Terraform provider, Agent, etc.)
- Where credentials come from (cross-account vs local)
- Whether ManagementConfig is present (push mode) or null (pull mode)
- Where StackSettings come from (CLI flags, Terraform attrs, Agent env vars, etc.)

StackSettings (user's choices) flows from user input into `DeploymentConfig`, then to controllers. It is separate from `StackState`, which tracks mutable runtime state.

## How It's Used

### Admin runs initial setup

```rust
// CLI
let mut current = initial_state;

loop {
    let result = step(current, config, credentials).await?;
    current = result.state;  // Direct assignment, no merge
    
    api.save(&agent_id, &current, result.update_heartbeat).await?;
    
    if let Some(delay_ms) = result.suggested_delay_ms {
        sleep(Duration::from_millis(delay_ms)).await;
    }
    
    if current.status == Running {
        break;
    }
}
```

### Continuous deployment loop

```rust
// Deployment loop (runs in CLI, Agent, or any other host)
loop {
    for deployment in query_deployments_needing_work() {
        let result = step(deployment.current, deployment.config, credentials).await?;

        // Save full state
        save_state(deployment.id, result.state, result.update_heartbeat).await?;
    }
    sleep(1s);
}
```

Same `step()` function, different callers.

## Relationship to alien-infra

One deployment phase spans many infra steps:

```
Provisioning (deployment status)
  → function-a: Pending → Provisioning (infra step)
  → function-a: Provisioning → Running (infra step)
  → function-b: Pending → Provisioning (infra step)
  → function-b: Provisioning → Running (infra step)
  → all Running → transition to Running (deployment status)
```

alien-deployment tracks the overall lifecycle (Provisioning). alien-infra tracks individual resources (function-a, function-b).

See `01-provisioning/00-infra.md` for how alien-infra provisions resources.

