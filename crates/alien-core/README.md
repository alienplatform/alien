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
write, or delete permission. The manager vault API refuses the deployer vault. Explicit management data grants
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

### Existing deployments

Rebuild prebuilt Worker images with the updated Worker runtime: the runtime
embedded in an older image still resolves the former vault. Then rerun setup to install the new vault and permissions, then rewrite keys at the
new names using your own cloud credentials. There is no fallback to former
locations. A missing key has the normal missing-slot report and write command.
Stored legacy values already shared with the control plane keep their existing
delivery.

### Indirect infrastructure privileges

This boundary covers direct value access. Infrastructure management still has
powers that can change IAM or deploy code using a workload identity:

- AWS management can create/update compute using stack roles and `iam:PassRole`.
  The reserved-vault deny applies to management itself, including parameter
  tags and resource policies; it does not follow a workload role. CloudTrail
  records `CreateFunction`, `UpdateFunctionCode`, `UpdateFunctionConfiguration`,
  `RunInstances`, and IAM `PutRolePolicy` / `UpdateAssumeRolePolicy` changes.
  `PassRole` is a permission check, not a separate CloudTrail event; inspect the
  role in the compute API event. KMS decrypt alone does not authorize SSM reads.
- GCP management retains project `setIamPolicy`, custom-role updates, and
  project-level secret creation. Secret creation checks the parent project,
  so IAM cannot exclude a requested slot name using a resource-name condition.
  IAM changes can grant subsequent payload access. Admin Activity logs record
  `SetIamPolicy`, `google.iam.admin.v1.UpdateRole`, and
  `google.cloud.secretmanager.v1.SecretManagerService.CreateSecret`.
- Azure management retains resource-group role-assignment writes and vault
  configuration powers. Those can grant payload access or change the vault's
  authorization model; infrastructure teardown can also delete the containing
  vault/resource group. Activity Log operations include
  `Microsoft.Authorization/roleAssignments/write`,
  `Microsoft.Authorization/roleDefinitions/write`,
  `Microsoft.KeyVault/vaults/write`, `Microsoft.KeyVault/vaults/accessPolicies/write`,
  and `Microsoft.KeyVault/vaults/delete`.

These are accepted infrastructure privileges, not a hard boundary against a
compromised infrastructure manager. Customer-owned deny policies (for example,
GCP IAM deny policies) and separately controlled provisioning identities are
possible follow-up work. On GCP/Azure, custom inline management permissions are
refused while native slots exist because they can embed arbitrary payload roles
or scopes; use reviewed named permission sets on concrete shared resources.
