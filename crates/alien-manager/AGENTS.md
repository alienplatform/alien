# alien-manager

Control plane for Alien applications. Mode-agnostic library with builder pattern.

## Architecture

- `builder.rs` — Builder with explicit trait-based providers; convenience `with_standalone_defaults()` for standalone SQLite setups
- `config.rs` — ManagerConfig (runtime settings), PlatformConfig, DeepStoreConfig, GcpOAuthConfig
- `server.rs` — AlienManager struct, start() method
- `bin/main.rs` — Binary entry point for standalone/platform wiring only
- `traits/` — Core trait definitions (DeploymentStore, ReleaseStore, TokenStore, etc.)
- `stores/sqlite/` — SQLite implementations of all traits
- `providers/` — Platform API providers, auth validators, credential resolvers
- `routes/` — Axum API routes
- `loops/` — Deployment loop, heartbeat loop, self-heartbeat loop

## Key Traits

- `DeploymentStore` — CRUD for deployments and deployment groups
- `ReleaseStore` — CRUD for releases
- `TokenStore` — Token management (create, validate, list)
- `CredentialResolver` — Cloud credential impersonation
- `TelemetryBackend` — Log/metric forwarding
- `AuthValidator` — Request authentication
- `ServerBindings` — KV, storage, command dispatcher

## Builder Pattern

`AlienManagerBuilder` accepts trait objects for each concern. The library has NO concept of "mode" — callers wire mode-specific implementations:

- **Standalone** (`bin/main.rs`) — SQLite stores, environment credentials, token-based auth
- **Platform** (`bin/main.rs` with `platform` feature) — API-backed providers, multi-tenant
- **Dev** (`alien dev`) — embedded by the CLI with explicit local providers, permissive auth, and in-memory telemetry

The builder requires all providers to be set explicitly. Convenience method `with_standalone_defaults()` sets up SQLite-backed standalone defaults (fills only unset ones). Dev wiring belongs in `alien-cli`, not here.

## Token Security

Tokens use SHA-256 hashed storage: the raw token is shown once at creation, then only `key_hash` + `key_prefix` are stored. Prefixes: `ax_admin_` for admin tokens, `ax_dg_` for deployment group tokens.

## Deployment Loop

`loops/deployment.rs` drives the deployment state machine:

1. Acquires deployments with active statuses via `DeploymentStore::acquire`
2. **Skips pull-mode deployments** — these are driven by alien-agent in-environment
3. **Skips push-mode creation/deletion phases** (`Pending`, `InitialSetup`, `DeletePending`, `Deleting`, `DeleteFailed`) — these run on the developer's machine via `alien-deploy-cli`
4. Resolves credentials, builds state, runs `alien_deployment::step()` in a loop
5. Uses `classify_status()` from `alien-deployment::loop_contract` to detect terminal states
6. Reconciles and releases locks unconditionally

## Registry Access Automation

`registry_access.rs` handles cross-account artifact registry access during sync/reconcile:

- **Grant** — when a deployment transitions toward or has reached `Provisioning`, grants IAM-based cross-account pull access (AWS ECR, GCP GAR)
- **Revoke** — when a deployment reaches `Deleted`, removes the access
- Best-effort: failures are logged but don't block deployment progress

## Providers

Key provider files:
- `permissive_auth.rs` — `alien dev`: accepts all requests
- `token_db_validator.rs` — Standalone: validates tokens against SQLite
- `local_credentials.rs` / `environment_credentials.rs` — Local and standalone credential resolution
- `platform_api/` — Platform mode providers that call the platform API
- `otlp_forwarding.rs` — Forwards telemetry to OTLP endpoint
- `null_telemetry.rs` / `in_memory_telemetry.rs` — No-op and local-dev telemetry

## Don't

- Don't add mode-specific logic to the library — it belongs in call sites (main.rs, alien dev, alien-platform-manager)
- Don't add a mode enum — the builder + traits IS the architecture. "Mode" is not a library concept.
- Don't add local-dev bootstrapping to the builder — `alien dev` should do it after startup
- Don't import `ExecutionMode` — that's CLI-only
- Don't skip token hashing — SHA-256 required (key_hash + key_prefix pattern)
- Don't use `am_` prefix for tokens — use `ax_admin_` for admin, `ax_dg_` for deployment group
- Don't reference platform/, deepstore/, or horizon/ — this is OSS code
