# alien-core

Core types shared across all Alien crates. Defines the vocabulary of the platform.

## Modules

- `stack` / `stack_state` / `stack_settings` — Stack definitions, resource graph, frozen vs live lifecycle
- `deployment/` — Deployment types: status, config, state, release, compute, domain, environment variables
- `platform` — Platform enum (Aws, Gcp, Azure, Kubernetes, Local)
- `resource` / `resources/` — Resource types and per-resource configs (Function, Container, Storage, KV, Queue, Vault, etc.)
- `bindings/` / `external_bindings` — Binding type definitions and external (bring-your-own) bindings
- `commands_types` — Remote command protocol types (CommandState, Envelope, BodySpec)
- `permissions` — Permission definitions
- `events/` / `app_events/` — Platform events (storage, queue, cron) and application-level events
- `sync` — Sync protocol types for pull-model deployments
- `embedded_config` — Embedded configuration for white-labeled binaries
- `image_rewrite` — OCI image URI rewriting for registry proxy
- `presigned` — Presigned URL types
- `dev_status` — `DevStatus` type for `alien dev` machine interface
- `client_config` — `ClientConfig` and `ImpersonationConfig` for cloud credential configuration
- `build_targets` — Binary target definitions (architecture, OS)
- `load_balancer` — Load balancer configuration types
- `instance_catalog` — Instance/machine type catalog

## Type Generation

Types are exported to TypeScript for `@alienplatform/core`:

```
Rust types (alien-core)
  → schema_exporter.rs (utoipa)
  → OpenAPI JSON
  → Kubb (packages/core/kubb.config.ts)
  → Zod schemas + TypeScript types
```

### Adding New Types

1. Define with `#[derive(Serialize, Deserialize)]` and `#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]`
2. Add to `schema_exporter.rs` in `#[openapi(components(schemas(...)))]`
3. Run `pnpm generate && pnpm build` from workspace root
4. Export from `packages/core/src/index.ts`
