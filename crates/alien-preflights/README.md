# alien-preflights

Pre-deployment checks and stack mutations. Validates configuration, checks cloud readiness, and modifies the stack to ensure successful deployment.

## Check Types

Registered in `PreflightRegistry::with_built_ins()`:

1. **`CompileTimeCheck`** — Validates stack config without cloud access (resource naming, dependencies, lifecycle rules)
2. **`RuntimeCheck`** — Validates cloud environment readiness with client access (feature-gated: `runtime-checks`)
3. **`StackCompatibilityCheck`** — Validates old vs new stack compatibility during updates
4. **`StackMutation`** — Modifies the stack to ensure deployment succeeds (adds platform resources, wires dependencies)

## Mutation Phases

Mutations execute in strict order. Each mutation's `should_run()` sees the stack as modified by prior mutations:

1. **Global infrastructure** — Network, Azure resource group
2. **Resource creation** — Container cluster, service accounts, secrets vault
3. **Service activations** — GCP/Azure API enablement, Container Apps environment, storage accounts
4. **Dependency wiring** — Service account dependencies, infrastructure dependencies (must be last)

## Adding Checks / Mutations

Implement the relevant trait, add to `PreflightRegistry::with_built_ins()` in the appropriate section. Mutations must only modify the stack based on configuration and state — no cloud queries.
