# alien-preflights

Preflight checks and stack mutations that run before deployment.

## Architecture

Four trait types, registered in `PreflightRegistry::with_built_ins()`:

1. **`CompileTimeCheck`** — Validates stack config without cloud access (resource naming, dependencies, lifecycle rules)
2. **`RuntimeCheck`** — Validates cloud environment readiness with client access (feature-gated: `runtime-checks`)
3. **`StackCompatibilityCheck`** — Validates old vs new stack compatibility during updates
4. **`StackMutation`** — Modifies the stack to ensure successful deployment (adds platform resources, wires dependencies)

## Mutation Phases

Mutations execute in strict order. Each mutation's `should_run()` sees the stack as modified by prior mutations:

1. **Global infrastructure** — Network, Azure resource group
2. **Resource creation** — Container cluster, service accounts, secrets vault
3. **Service activations** — GCP/Azure API enablement, Container Apps environment, storage accounts
4. **Dependency wiring** — Service account dependencies, infrastructure dependencies (must be last)

## Key Files

- `lib.rs` — `PreflightRegistry`, `CheckResult`, `PreflightSummary`, all trait definitions
- `runner.rs` — Executes checks and mutations in order
- `compile_time/` — All compile-time checks (resource validation, naming, lifecycle)
- `compatibility/` — Frozen resources unchanged, permission profiles unchanged
- `mutations/` — Per-platform mutations (network, service accounts, vault, service activations)
- `runtime/` — Runtime checks (cloud connectivity, quotas — placeholder)

## Adding a Check

1. Implement `CompileTimeCheck` (or `RuntimeCheck`/`StackCompatibilityCheck`)
2. Add to `PreflightRegistry::with_built_ins()` in the appropriate section
3. Return `CheckResult::success()`, `CheckResult::failed(errors)`, or `CheckResult::with_warnings(warnings)`

## Adding a Mutation

1. Implement `StackMutation` with `description()`, `should_run()`, `mutate()`
2. Add to `PreflightRegistry::with_built_ins()` in the correct phase
3. Mutations should only modify the stack based on stack configuration and stack state — no cloud queries

## Don't

- Don't put mutations in the wrong phase — dependency wiring must be last
- Don't query cloud APIs in mutations — they modify the stack only
- Don't use "agent" — use "deployment"
