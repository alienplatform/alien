# alien-core

Core types shared across all Alien crates. Defines the vocabulary of the platform.

## Modules

- `stack` / `stack_state` / `stack_settings` — Stack definitions, resource graph, frozen vs live lifecycle
- `deployment/` — Deployment types: status, config, state, release, compute, domain, environment variables
- `platform` — Platform enum (Aws, Gcp, Azure, Kubernetes, Local)
- `resource` / `resources/` — Resource types and per-resource configs (Worker, Container, Storage, KV, Queue, Vault, etc.)
- `bindings/` / `external_bindings` — Binding type definitions and external (bring-your-own) bindings
- `commands_types` — Remote command protocol types (CommandState, Envelope, BodySpec)
- `permissions` — Permission definitions
- `events/` / `app_events/` — Platform events (storage, queue, cron) and application-level events
- `sync` — Sync protocol types for pull-model deployments
- `embedded_config` — Embedded configuration for pre-configured / rebranded binaries
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

## Deployer secrets

A secret input provided by the deployer is stored in the reserved, setup-owned
`deployer-secrets` vault. Its key is the input ID in kebab case: `apiKey` becomes
`api-key`. The deployer writes it with their own cloud credentials using the
command in deployment status. AWS SSM and GCP Secret Manager names are
`<prefix>-deployer-secrets-api-key`; Kubernetes uses that Secret name with data
key `value`. Azure uses secret `api-key` in a separate Key Vault whose generated
name is reported by the deployment.

The `secrets` vault remains for shared developer environment secrets and stored
legacy input values. Management keeps its existing access there. Management
receives metadata access to deployer slots, with no direct secret-value read,
write, or delete permission. The manager vault API refuses deployer slots,
including their former shared-vault names. Explicit management data grants
cannot reopen the reserved vault.

AWS and GCP workloads read only their mapped slots. Workloads sharing one
permission profile must receive the same slots. Azure grants those workloads
read access to the dedicated vault: per-secret RBAC cannot be installed before
the secret exists. Kubernetes workloads use `secretKeyRef`; the in-cluster
Operator needs Secret `get` for presence checks, and Kubernetes has no
metadata-only Secret permission. Local development uses the local file vault.

This separates direct management access from workload access. It does not
prevent trusted workload code from using its secrets, or an infrastructure
administrator from changing IAM or workload code.

### Moving an existing vault-native slot

Rerun setup to install the new vault and permissions. With your own cloud
credentials, write the existing value using the new command in deployment
status, then delete the former `<prefix>-secrets-input-api-key` value (or
`input-api-key` in the former Azure Key Vault). Missing-slot reports include the
write and delete commands. Alien never copies values or reads the old location
as a fallback; an old-name-only slot remains missing. Stored legacy values
already shared with the control plane keep their existing delivery.
