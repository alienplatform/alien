# Permissions

## Declaring Permissions

This is how developers define permissions in their Alien stack:

```typescript
const logsStorage = new alien.Storage("logs-storage").build()
const codeStorage = new alien.Storage("code-storage").build()

const myFunction = new alien.Function("my-function")
  .permissions("reader")  // Uses the "reader" profile
  .build()

export default new alien.Stack("my-app")
  .add(logsStorage, "frozen")
  .add(codeStorage, "frozen")
  .add(myFunction, "live")
  .permissions({
    profiles: {
      reader: {
        "*": ["storage/data-read"],             // All storage in the stack
        "logs-storage": ["storage/data-write"]  // Extra permissions on this specific bucket
      }
    }
  })
  .build()
```

Each key in `profiles` is a **permission profile** — a named identity that compute resources can assume. Under the hood, each profile becomes a service account in the target platform (IAM Role on AWS, Service Account on GCP, Managed Identity on Azure).

The keys inside each profile are **scopes**:
- `"*"` — stack-level: applies to all resources with the stack prefix
- `"logs-storage"` — resource-scoped: applies only to this specific frozen resource

The values are **permission sets** — named bundles of permissions like `storage/data-read`.

The advantages:

1. **Single source of truth** — all permissions are declared in one place
2. **Clarity of scope** — global (`"*"`) vs resource-scoped is explicit
3. **Least privilege by default** — nothing is granted unless declared

## Permission Sets

A permission set is a named bundle of permissions that works across all platforms. Alien includes built-in sets for common operations:

| Category | Permission Sets |
|----------|-----------------|
| storage | `data-read`, `data-write`, `management`, `provision`, `heartbeat` |
| function | `execute`, `invoke`, `management`, `provision`, `heartbeat` |
| queue | `data-read`, `data-write`, `management`, `provision`, `heartbeat` |
| vault | `data-read`, `data-write`, `management`, `provision`, `heartbeat` |
| kv | `data-read`, `data-write`, `management`, `provision`, `heartbeat` |
| build | `execute`, `logs-and-artifacts`, `management`, `provision`, `heartbeat` |
| container-cluster | `execute`, `management`, `provision`, `heartbeat` |

The naming convention:
- `data-read` / `data-write` — application-level access to data
- `execute` — runtime permissions for compute (logs, pull images)
- `invoke` — calling functions from outside
- `management` — read + update (used for live resources during ongoing operations)
- `provision` — full lifecycle: create, update, delete (used during initial setup for all resources)
- `heartbeat` — read-only monitoring (used for frozen resources during ongoing operations)

Developers use these built-in sets 99% of the time. For the other 1%, they can define custom permission sets inline.

## What's Inside a Permission Set?

Each permission set is a JSONC file that defines what to grant and how to bind it — per platform:

```jsonc
// permission-sets/storage/data-read.jsonc
{
  "id": "storage/data-read",
  "description": "Allows reading data from storage buckets and containers",
  "platforms": {
    "aws": [{
      "grant": {
        "actions": ["s3:GetObject", "s3:GetObjectVersion", "s3:ListBucket"]
      },
      "binding": {
        "stack": {
          "resources": [
            "arn:aws:s3:::${stackPrefix}-*",
            "arn:aws:s3:::${stackPrefix}-*/*"
          ]
        },
        "resource": {
          "resources": [
            "arn:aws:s3:::${resourceName}",
            "arn:aws:s3:::${resourceName}/*"
          ]
        }
      }
    }],
    "gcp": [{
      "grant": {
        "permissions": ["storage.objects.get", "storage.objects.list", "storage.buckets.get"]
      },
      "binding": {
        "stack": {
          "scope": "projects/_/buckets/${resourceName}"
        },
        "resource": {
          "scope": "projects/_/buckets/${resourceName}"
        }
      }
    }],
    "azure": [{
      "grant": {
        "dataActions": [
          "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read"
        ]
      },
      "binding": {
        "stack": {
          "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}"
        },
        "resource": {
          "scope": "/subscriptions/${subscriptionId}/.../storageAccounts/${resourceName}"
        }
      }
    }]
  }
}
```

Two parts:
- **Grant** — what to allow (AWS actions, GCP permissions, Azure actions/dataActions)
- **Binding** — where to apply (stack-wide with prefix patterns, or specific resource)

Variables like `${stackPrefix}` and `${resourceName}` get replaced at deployment time.

## What Gets Generated

When you deploy to AWS, the permission set above generates this IAM policy:

**Stack-level binding** (for `"*": ["storage/data-read"]`):

```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Sid": "StorageDataRead",
    "Effect": "Allow",
    "Action": ["s3:GetObject", "s3:GetObjectVersion", "s3:ListBucket"],
    "Resource": ["arn:aws:s3:::my-app-*", "arn:aws:s3:::my-app-*/*"]
  }]
}
```

**Resource-level binding** (for `"logs-storage": ["storage/data-write"]`):

```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Sid": "StorageDataWrite",
    "Effect": "Allow",
    "Action": ["s3:PutObject", "s3:DeleteObject"],
    "Resource": ["arn:aws:s3:::my-app-logs-storage", "arn:aws:s3:::my-app-logs-storage/*"]
  }]
}
```

On GCP, the same permission set generates:
1. A custom role with the permissions
2. Resource-level IAM bindings via `setIamPolicy` on individual resources (for both stack-level and resource-level scope)
3. One GCP project per stack — no CEL conditions needed since all resources in the project belong to the stack

On Azure:
1. A custom role definition
2. A role assignment at resource group scope for stack-level
3. A role assignment on the resource for resource-level

## Custom Permission Sets

For edge cases, developers can define inline permission sets:

```typescript
const assumeAnyRole: PermissionSet = {
  id: "assume-any-role",
  platforms: {
    aws: [{
      grant: { actions: ["sts:AssumeRole"] },
      binding: {
        stack: {
          resources: ["*"],
          condition: { StringEquals: { "sts:ExternalId": "my-ext-id" } }
        }
      }
    }]
  }
}

export default new alien.Stack("my-app")
  .permissions({
    profiles: {
      execution: {
        "*": ["storage/data-read", assumeAnyRole],
      }
    }
  })
  .build()
```

## How It Works Internally

### From Profile to ServiceAccount

During deployment preflights, each profile becomes a `ServiceAccount` resource:

```
Profile "reader" → ServiceAccount "reader-sa"
```

The ServiceAccount contains resolved permission sets (stack-level only — resource-scoped permissions are handled separately).

On each platform, ServiceAccount compiles to:
- **AWS**: IAM Role
- **GCP**: Service Account  
- **Azure**: User-assigned Managed Identity

### Who Can Assume the ServiceAccount?

The ServiceAccount controller analyzes the stack to build the trust policy. If a Function uses the "reader" profile, the trust policy allows Lambda to assume the role:

```json
{
  "Version": "2012-10-17",
  "Statement": [{
    "Effect": "Allow",
    "Principal": { "Service": "lambda.amazonaws.com" },
    "Action": "sts:AssumeRole"
  }]
}
```

If a Build resource uses the profile, CodeBuild is added. If another ServiceAccount needs to impersonate this one (via `service-account/impersonate`), that role ARN is added.

For Container workloads running on VMs, the trust policy also includes the ContainerCluster's VM role. This allows the IMDS metadata proxy running on each VM to assume per-container service account roles, delivering container-specific credentials at runtime. See [Cloud Identity](07-containers/11-cloud-identity.md) for details.

### Applying Stack-Level Permissions

The ServiceAccount controller generates and attaches the IAM policy:

```
CreatingRole → ApplyingStackPermissions → Ready
```

1. Create IAM role with trust policy
2. Generate combined policy from all stack permission sets
3. Attach policy to role via `put_role_policy`

### Applying Resource-Scoped Permissions

Resource controllers apply resource-scoped permissions after creating the resource.

**AWS**: Adds statements to the role's policy with specific resource ARNs.

**GCP**: Each resource controller calls `setIamPolicy` on the resource:

```rust
ResourcePermissionsHelper::apply_gcp_resource_scoped_permissions(
    ctx,
    "logs-storage",    // Resource ID in stack
    bucket_name,       // Actual GCS bucket name
    "storage",
    bucket,
    |bucket, policy| bucket.set_iam_policy(policy),
).await?;
```

**Azure**: Creates role assignments scoped to the specific resource.

## Management Permissions

Management permissions control what the managing account can do in the remote environment — update functions, check health, and optionally create resources.

### Frozen vs Live Is a User Choice

When adding resources to a stack, the developer chooses whether each resource is `frozen` or `live`:

```typescript
export default new alien.Stack("my-app")
  .add(dataStorage, "frozen")   // Developer decides this is frozen
  .add(codeStorage, "frozen")   // Developer decides this is frozen
  .add(myFunction, "live")      // Developer decides this is live
  .build()
```

This is not an inherent property of the resource type. A `Storage` resource could be frozen or live depending on the use case. A storage bucket for long-lived logs is typically frozen (created once, rarely changed). A storage bucket that gets recreated with each deployment could be live.

The choice affects two things:
1. **What happens after initial setup** — all resources are created during initial setup, but only live resources are updated during ongoing operations. Frozen resources remain untouched.
2. **What management permissions are needed** — frozen resources only need heartbeat monitoring, live resources need management + heartbeat permissions

### Auto-Generation

By default, management permissions are auto-generated based on resource lifecycles:

```
Frozen resources → <type>/heartbeat     (read-only monitoring)
Live resources   → <type>/management    (read + update)
                 + <type>/heartbeat     (read-only monitoring)
```

The `provision` permission set (`<type>/provision`) is **never** auto-generated. It grants full lifecycle permissions (create, update, delete) and is only needed during initial setup, which runs with elevated credentials. After initial setup, the managing account operates with least-privilege permissions — it can update live resources but not create or delete them.

The `ManagementPermissionProfileMutation` calculates this during preflights.

### Customization

Developers can extend the auto-generated permissions using `ManagementPermissions.extend()`. This is the way to explicitly opt into `provision` permissions if the managing account needs to create or delete resources after initial setup:

```typescript
.permissions({
  management: ManagementPermissions.extend({
    "*": ["storage/provision"]  // Explicitly grant provision for all storage
  })
})
```

Or override completely:

```typescript
.permissions({
  management: ManagementPermissions.override({
    "*": ["storage/management", "function/management"]
  })
})
```

### Who is the Manager?

On AWS, GCP, and Azure, stacks are managed by another cloud account (cross-account access). There's no agent running in the customer's cloud that could fail.

The `RemoteStackManagement` resource handles cross-account setup:

**AWS**:
1. Create IAM role in target account
2. Trust policy allows the managing account's role to assume it
3. Attach management permissions as inline policy

**GCP**:
1. Create service account in target project
2. Grant `roles/iam.serviceAccountTokenCreator` to managing service account
3. Create custom role with management permissions
4. Bind custom role to the service account

**Azure** (via UAMI + FIC + custom RBAC):
1. Create User-Assigned Managed Identity (UAMI) in customer's resource group
2. Create Federated Identity Credential (FIC) on the UAMI, trusting the manager's OIDC issuer
3. Create custom role definition from `/provision` permission sets at RG scope
4. Create role assignments binding the custom role to the UAMI principal

For Kubernetes and Local platforms, an Agent runs inside the environment and pulls configuration. No cross-account access needed.

## Platform Differences

| Aspect | AWS | GCP | Azure |
|--------|-----|-----|-------|
| Identity | IAM Role | Service Account | Managed Identity |
| Stack-level scope | ARN wildcards (`my-stack-*`) | Resource-level IAM on each resource | Resource group scope |
| Resource-level scope | Specific ARN in role policy | `setIamPolicy` on individual resource | Role assignment on specific resource |
| Cross-account | AssumeRole with trust policy | Service account impersonation | OIDC token exchange via FIC |
| Provision scope | Wildcard ARNs (resources don't exist yet) | Project-level (resources don't exist yet) | Resource-group-scoped (resources don't exist yet) |

**AWS**: All permissions go into IAM role policies. Stack-level uses wildcard ARNs. Resource-level uses specific ARNs. Both end up in the same policy document. For management permissions, resource-specific ARN statements are used (not wildcards), except for `provision` which must use wildcards since the resources don't exist yet.

**GCP**: Permissions go into custom roles. IAM bindings are applied via `setIamPolicy` on individual resources — one GCP project per stack, so no CEL conditions are needed. For management permissions, resource-level IAM is applied on each resource. `provision` permissions use project-level bindings since the resources don't exist yet.

**Azure**: Permissions go into custom role definitions. Resource-level creates role assignments on specific resources. For management permissions, role assignments are scoped to specific resources (not resource-group-scoped), except for `provision` which uses resource-group scope since the resources don't exist yet.

## The Permission Generators

`alien-permissions` provides generators that translate permission sets to platform-specific formats:

**AwsRuntimePermissionsGenerator** — IAM policies with literal values:
```rust
generator.generate_policy(&permission_set, BindingTarget::Stack, &context)?
// → AwsIamPolicy with Resource: ["arn:aws:s3:::my-stack-*"]
```

**AwsCloudFormationPermissionsGenerator** — IAM policies with CloudFormation intrinsics:
```rust
generator.generate_policy(&permission_set, BindingTarget::Stack, &context)?
// → Resource: [{"Fn::Sub": "arn:aws:s3:::${AWS::StackName}-*"}]
```

**GcpRuntimePermissionsGenerator** — Custom roles and IAM bindings:
```rust
generator.generate_custom_role(&permission_set, &context)?
generator.generate_bindings(&permission_set, BindingTarget::Stack, &context)?
```

**AzureRuntimePermissionsGenerator** — Role definitions and assignments:
```rust
generator.generate_role_definition(&permission_set, binding_target, &context)?
generator.generate_role_assignment(&permission_set, binding_target, &context)?
```

## Adding New Permission Sets

1. Create a JSONC file in `alien-permissions/permission-sets/<category>/<name>.jsonc`

2. Define grants and bindings for each platform:

```jsonc
{
  "id": "my-resource/my-action",
  "description": "Allows my action on my resource",
  "platforms": {
    "aws": [{
      "grant": { "actions": ["myservice:MyAction"] },
      "binding": {
        "stack": { "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:resource/${stackPrefix}-*"] },
        "resource": { "resources": ["arn:aws:myservice:${awsRegion}:${awsAccountId}:resource/${resourceName}"] }
      }
    }]
  }
}
```

3. Rebuild. The build script compiles all permission sets into a static registry:

```rust
alien_permissions::get_permission_set("my-resource/my-action")
```

## Variables Reference

| Variable | Description | Example |
|----------|-------------|---------|
| `${stackPrefix}` | Stack name | `my-app` |
| `${resourceName}` | Full resource name | `my-app-logs-storage` |
| `${awsRegion}` | AWS region | `us-east-1` |
| `${awsAccountId}` | AWS account | `123456789012` |
| `${managingAccountId}` | Manager's AWS account | `987654321098` |
| `${projectName}` | GCP project | `my-gcp-project` |
| `${region}` | GCP region | `us-central1` |
| `${subscriptionId}` | Azure subscription | `00000000-0000-...` |
| `${resourceGroup}` | Azure resource group | `my-app-rg` |

## Key Files

| File | Purpose |
|------|---------|
| `alien-core/src/permissions.rs` | Permission types: `PermissionSet`, `PermissionProfile`, `ManagementPermissions` |
| `alien-permissions/permission-sets/` | Built-in permission set JSONC files |
| `alien-permissions/src/registry.rs` | Compile-time registry of permission sets |
| `alien-permissions/src/generators/` | Platform-specific generators |
| `alien-preflights/src/mutations/service_account.rs` | Creates ServiceAccounts from profiles |
| `alien-preflights/src/mutations/management_permission_profile.rs` | Auto-generates management permissions |
| `alien-infra/src/service_account/` | ServiceAccount controllers per platform |
| `alien-infra/src/remote_stack_management/` | Cross-account management controllers |
| `alien-infra/src/core/resource_permissions_helper.rs` | Helper for resource-scoped permissions |
