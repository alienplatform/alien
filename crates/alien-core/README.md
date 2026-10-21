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
`api-key`. User vault IDs cannot overlap `deployer-secrets`: this reserves the exact ID,
its `deployer-secrets-` descendants, and the ancestor `deployer` (including normalized aliases). The deployer writes it with their own cloud credentials using the
command in deployment status. AWS SSM and GCP Secret Manager names are
`<prefix>-deployer-secrets-api-key`; Kubernetes uses that Secret name with data
key `value`. Azure uses secret `api-key` in a separate Key Vault whose generated
name is reported by the deployment.

The `secrets` vault remains for shared developer environment secrets and stored
legacy input values. Management keeps its existing access there. Management
receives metadata access to deployer slots, with no direct secret-value read,
write, or delete permission. The manager vault API refuses the deployer vault. Explicit management data grants
cannot reopen the reserved vault.

For declared deployer inputs, AWS and GCP workloads receive only their mapped reads. Workloads sharing one
permission profile must receive the same slots. Stack-wide workload
`vault/data-read`, `vault/data-write`, and `vault/provision` grants remain
supported. On AWS each such profile is denied all writes, deletion, version labels,
tags, and resource-policy changes under the entire deployer-slot prefix, including
future slots. This guard is installed even before the first native slot is declared.
Read denies cover every declared deployer input not mapped into the profile,
including stored-value inputs. Parameters left behind by removed or renamed inputs
are outside this exact read deny; customers must delete those old parameters.
Root-path `GetParametersByPath` requests are denied because they can return these
root-level slots; reads of other hierarchies such as `/myapp/config` remain available
when otherwise granted. [AWS authorizes path reads against the requested path](https://docs.aws.amazon.com/systems-manager/latest/userguide/ps-retrieval-authorization.html).

GCP ordinary version-read, version-write and delete grants exclude the dedicated
prefix. Secret creation is checked on the parent project and can create an empty
secret under any name; it does not include a payload. Adding a version separately
requires permission on that secret, so the ordinary data-write grant cannot fill
an existing or future deployer slot. A workload with `vault/provision` also has
project IAM-change powers and can grant itself broader access; that existing grant is an
infrastructure-administrator capability, not a payload-access boundary. [Create](https://docs.cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/create)
and [addVersion](https://docs.cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/addVersion)
use these separate authorization checks. Explicit workload grants on overlapping
vault prefixes are rejected; use input mappings for slot access.

Azure grants mapped workloads read access to the entire dedicated vault:
per-secret RBAC cannot be installed before the secret exists. A workload with
stack-wide vault data permissions can also reach this vault on Azure, even
without a mapped slot, including writing a future slot when granted data-write.
Scope ordinary Azure workload grants to a specific vault
when that access is unwanted. Kubernetes workloads use `secretKeyRef`; the
in-cluster Operator needs Secret `get` for presence checks, and Kubernetes has
no metadata-only Secret permission. Azure certificate imports and deletion are
scoped to the shared vault, because certificates have backing secrets. Vault
provisioning grants only vault lifecycle actions, without ARM secret child-resource
writes. Local development uses the local file vault.

This separates direct management access from workload access. It does not
prevent trusted workload code from using its secrets, or an infrastructure
administrator from changing IAM or workload code.

### Existing deployments

Rebuild prebuilt Worker images with the updated Worker runtime: the runtime
embedded in an older image still resolves the former vault. For AWS and GCP,
write keys at the new names with your own cloud credentials, then rerun setup
to install the new permissions. For Azure, rerun setup first to create the
separate Key Vault, then write keys using the reported vault name. An existing
Running deployment without the dedicated vault reports missing slots with
setup guidance; health checks keep it Running. There is no fallback to former
locations. After setup, an absent key gets the normal missing-slot report and
write command.
Stored legacy values already shared with the control plane keep their existing
delivery.

### Indirect infrastructure privileges

This boundary covers direct value access. Infrastructure management still has
powers that can change IAM or deploy code using a workload identity:

- AWS management can create/update compute using stack roles and `iam:PassRole`.
  Management is explicitly denied `sts:AssumeRole` on stack roles, including
  a self-trusting role it creates; this prevents role chaining without compute.
  Automatic setup keeps data access to ordinary Live vaults added later, while
  the explicit reserved-vault deny overrides its stack-wide Allow.
  The reserved-vault deny applies to management itself, including parameter
  tags and resource policies; it does not follow a workload role. CloudTrail
  records `AssumeRole`, `CreateRole`, `CreateFunction`, `UpdateFunctionCode`, `UpdateFunctionConfiguration`,
  `RunInstances`, and IAM `PutRolePolicy` / `UpdateAssumeRolePolicy` changes.
  `PassRole` is a permission check, not a separate CloudTrail event; inspect the
  role in the compute API event. KMS decrypt alone does not authorize SSM reads.
- GCP management, after a Terraform setup, holds no permission that changes
  IAM in a way that reaches the secrets vaults: no project, service-account or
  secret `setIamPolicy`, and no custom-role edits. It cannot grant itself a
  read or make itself able to impersonate a workload. It keeps
  `iam.serviceAccounts.actAs` to deploy workloads, the same class as AWS
  `iam:PassRole`, and project-level secret creation. Secret creation checks
  the parent project, so IAM cannot exclude a requested slot name using a
  resource-name condition. Automatic setup grants every provision set,
  including project `setIamPolicy` and custom-role updates, so there IAM
  changes can grant subsequent payload access. A hard boundary for automatic
  setup needs a customer-owned organization IAM deny policy:
  `roles/iam.denyAdmin` cannot be granted on a project. Admin Activity logs
  record `SetIamPolicy`, `google.iam.admin.v1.UpdateRole`, and
  `google.cloud.secretmanager.v1.SecretManagerService.CreateSecret`.
- Azure setup creates every custom role the runtime assigns, for management
  and for each permission profile, and management holds no
  `roleDefinitions/write`. Management's role-assignment writes sit in a role of
  their own, assigned with a condition that admits two pairs only: a declared
  management role to the management identity, and a declared application role
  to one of the deployment's own workload identities. The built-in Key Vault and
  Service Bus roles the controllers assign are listed the same way. It cannot give itself any other
  role, assign a built-in such as Owner, or grant a role to an identity it
  created. A condition cannot constrain scope, so a declared role can be
  assigned at the resource group rather than on one resource. Azure treats a
  conditioned role-assignment write as restricted, so management also cannot
  switch a Key Vault from RBAC to access policies. It keeps vault configuration
  and teardown powers, Managed Identity Contributor, and it can still deploy
  code that runs as a workload identity. A release that needs a new role or a
  new workload identity waits for setup to run again. Self-assignments the
  runtime made before this are removed when that resource is next reconciled,
  except one that grants the same role at the same scope as a declared grant,
  which is kept. Activity Log operations include
  `Microsoft.Authorization/roleAssignments/write`,
  `Microsoft.Authorization/roleDefinitions/write`,
  `Microsoft.KeyVault/vaults/write`, and `Microsoft.KeyVault/vaults/delete`.

These are accepted infrastructure privileges, not a hard boundary against a
compromised infrastructure manager. Customer-owned deny policies (for example,
GCP IAM deny policies) and separately controlled provisioning identities are
possible follow-up work. On GCP/Azure, custom inline management permissions are
refused while native slots exist because they can embed arbitrary payload roles
or scopes; use reviewed named permission sets on concrete shared resources.
