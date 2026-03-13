# Alien Permissions - Design

This document outlines the design for the cloud-agnostic Alien permissions system. It's based on role-based access control (RBAC).

## Permission sets

In Alien, a `PermissionSet` is a JSON object that contains the permissions to grant (the RBAC bundle) and binding instructions per platform:

```json
{
  "id": "storage/data-read",
  "description": "Allows reading data from storage resources",
  "platforms": {
    "aws": {
      "grant": {
        "actions": [
          "s3:GetObject",
          "s3:GetObjectVersion",
          "s3:ListBucket"
        ]
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
    },
    "gcp": {
      "grant": {
        "permissions": [
          "storage.objects.get",
          "storage.objects.list",
          "storage.buckets.get"
        ]
      },
      "binding": {
        "stack": {
          "scope": "projects/${projectName}",
          "condition": {
            "title": "Stack-prefixed only",
            "expression": "resource.name.startsWith('projects/_/buckets/${stackPrefix}-')"
          }
        },
        "resource": {
          "scope": "projects/_/buckets/${resourceName}"
        }
      }
    },
    "azure": {
      "grant": {
        "actions": [
          "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read"
        ],
        "dataActions": []
      },
      "binding": {
        "stack": {
          "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}"
        },
        "resource": {
          "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}/providers/Microsoft.Storage/storageAccounts/${storageAccountName}"
        }
      }
    }
  }
}
```

A permission set has a name (e.g. `storage/data-read`) and can be referenced.

The permission set defines what to grant and instructions on where to bind it (binding). These binding instructions are not the actual assignments.

Not all platforms are required. If a permission set isn't available for a platform, it doesn't translate to anything in that platform. 

Alien comes with built-in permission sets such as:

 - storage/data-read
 - storage/data-readwrite
 - storage/management
 - storage/provision
 - function/execute
 - function/management
 - function/provision

Note that DX is extremely important and users will probably use these built-in permission sets 99% of the time. 

## Defining permissions declaratively

This is how users define permissions in their Alien stack:

```typescript
// Create a function that uses the "reader" profile
const myFunction = new alien.Function("my-function")
  .memoryMb(1024)
  .permissions("reader")
  .build()

// Frozen resources can be targeted by resource-scoped permissions
const logsStorage = new alien.Storage("logs-storage").build()
const codeStorage = new alien.Storage("code-storage").build()

const stack = new alien.Stack("my-app")
  .add(logsStorage, "frozen")
  .add(codeStorage, "frozen")
  .add(myFunction, "live")
  .permissions({
    reader: {
      "*": ["storage/data-read"],             // global to all resources
      "logs-storage": ["storage/data-write"]  // extra permissions only on this resource
    },
    management: {
      "*": ["function/management", "storage/management"]
    }
  })
```

Each key in `stack.permissions({ ... })` is a **profile** - a named, non-human identity a compute service can assume (e.g., Lambda, Cloud Run, ECS, Container Apps). Under the hood, a profile maps to a service account in the target cloud.

The key of the profile is an Alien scope. It can be `*` for any resource in the stack, or `<name>` for a specific frozen resource, like `logs-storage`. Note that resource-scoped permissions cannot be applied on live resources. The value can be the name of a permission set (e.g. `storage/data-read`), or an actual custom permission set.

The advantages:

  1. **Single source of truth** - There's a single place to see and review *all* permissions. 
  2. **Clarity of scope** — global ("*") vs. resource-scoped is explicit.
  3. **Least privilege by default** — nothing is granted unless declared.

## Custom permission sets

`stack.permissions(...)` can easily be extended with custom permission sets like this:

```typescript

const assumeAnyRole: PermissionSet = {
  id: "assume-any-role",
  platforms: {
    aws: {
      grant: { actions: ["sts:AssumeRole"] },
      binding: {
        stack: {
          resources: ['*'],
          condition: { StringEquals: { "sts:ExternalId": "my-ext-id" } }
        }
      }
    }
  }
}

const stack = new alien.Stack("my-app")
  .permissions({
    execution: {
      "*": ["storage/data-read", assumeAnyRole],
      "logs-storage": ["storage/data-write"], // extra permissions only on this resource
    }
  })
```

Note that references to permission sets are resolved in the stack processor step, during build (see alien-build/stack_processor.rs).


## `alien-infra` integration

We would like to use `alien-infra`'s existing resource dependency graph. Therefore, we introduce a new semi-internal resource that is created automatically for the user.

### `ServiceAccount`

A `ServiceAccount` in Alien represents a non-human identity that can be assumed by compute services such as Lambda, Cloud Run, ECS, Container Apps, etc.

```typescript
const reader = new alien.ServiceAccount("reader")
  .stackPermissionSet({
    "id": "...",
    "description": "...",
    // Must be an actual permission set, not a reference.
  })
  .stackPermissionSet({ ... })
  .stackPermissionSet({ ... })
  .stackPermissionSet({ ... })
  .build()
```

A `ServiceAccount` is created automatically for each profile in `stack.permissions({ ... })`.

It compiles to a `Role` in AWS, `ServiceAccount` in Google Cloud, and a `User-assigned Managed Identity` in Azure.

`.stackPermissionSet(...)` is used to add permission sets that apply on all the resources in the stack (this is the `*` key in the permissions profile).

The resource controller is responsible to add these permissions on the entire stack, per the instructions in the `binding` field of the permission set. Usually, in AWS and GCP this is based on the resource name prefix, and in Azure this is just the resource group specifically created for the stack.

### Resource controller integration

Resource controllers (e.g. AWS Storage, GCP Artifact Registry, etc.), are responsible to apply IAM policies on the resource after creation. 

This is extremely useful in platforms like GCP where there's an entirely different API in each service for applying resource policies, and straightforward in AWS and Azure.

All resource controllers have access to the `Stack` object, which contains the permissions. It can iterate all profiles, find the relevant permission sets by the resource name, and start applying all permission sets. 

### CloudFormation generator

`alien-infra` currently contains code to generate a CloudFormation template from an Alien Stack. In the future, we'd like to separate this code to another create `alien-cloudformation`. 

The generator can iterate the Stack's profiles and generate a `AWS::IAM::Role` object. It can then start building policies from there. Variables like `${stackPrefix}` should be resolved to the CloudFormation's stack name.

## Management 

### Management permission set 

In the new design, we'll treat the "management" permissions profile as special. It defines the permission set needed to manage the stack. The user can specify it manually like this:

```typescript
const stack = new alien.Stack("my-app")
  .permissions({
    // other profiles

    management: {
      "*": ["function/provision", "storage/management", ...]
    }
  })
```

However, this profile can also be derived automatically, if not specified. The logic to automatically derive it is this:

```typescript
{
  "*": [
    // iterate all frozen resources, and for each one of them add:
    "<resourceType>/management"

    // iterate all live / liveOnSetup resources, and for each one of them add:
    "<resourceType>/provision"
  ],
}
```

We basically provide management access to all frozen resources, and provisioning access to all live resources.

To simplify, the management profile currently does not support resource-scoped permissions. 


### Who is the manager?

When an Alien stack is deployed on AWS, GCP, or Azure platforms, it's managed by another cloud account. There's no 'agent' in the customer's cloud that could fail and lead to failures in updates. 

For on-prem deployments and other restricted environments where cross-account access is impossible, we use the Kubernetes or Local platforms. In Kubernetes, there's an operator responsible to pull the stack configuration and apply it. In Local, it's similar, we have the "Local Alien Cloud" that pulls the stacks and applies it. In the future we might combine the Kubernetes + Local platforms.

Note: Previously we had StackManagement::Account and StackManagement::Function - we need to delete it, it's irrelevant from now.

So now, let's discuss AWS/GCP/Azure:

  - GCP: 
    0. We assume there's a service account in the management account (`mgmt-sa`).
    1. We create a service account in the target account (`target-sa`).
    2. We grant the `management` permission set to `target-sa`. 
    3. On `target-sa`, we grant `roles/iam.serviceAccountTokenCreator` and `roles/iam.serviceAccountUser` to `mgmt-sa`. 

  - AWS:
    0. We assume there's a role in the management account (`mgmt-sa`).
    1. We create a role in the target account, with trust policy that enables `mgmt-sa` to assume this role. 
    2. We grant the `management` permission set to the cross-account role.

  - Azure (via Lighthouse):
    0. We assume there's a user-assigned managed identity in the management account (`mgmt-sa`)
    1. We create Lighthouse registration definition in the target account to `mgmt-sa` with the `management` permission set 
    2. We create a Lighthouse registration assignment in the target account 

To make it easy with all these differences, on the AWS/GCP/Azure platforms we'll add a new `RemoteManagementAccess` resource in stack_processor, similarly to how we add a `ServiceAccount` for each permissions profile. The respective controller (e.g. AwsRemoteManagementAccessController etc) will be responsible to create everything. 

Since this is a special resource and unrelated to the `ServiceAccount` resource we've discussed before, and since we've simplified the `management` profile and disallowed resource-scoped permissions, we don't need to other resource controllers to target it when applying permissions. 

### Thoughts about various edge cases

#### 1. AWS role trust policy

AWS role needs a trust policy based on who can assume it (e.g. Lambda service, Codebuild service, another account).

This can simply be calculated by looking at the Stack, and seeing who needs to assume the service account (which is translated to Role in AWS).
If there's a Function resource, then the trust policy should include Lambda service, if there's a Build resource, then the trust policy should include Codebuild service ,etc.

#### 2. ECR access

`Function` resources are translated to AWS Lambda functions. Before, we had to do various hacks like this:

```
fn get_cross_service_permissions(
    &self,
    permission_level: PermissionLevel,
    managing_account_id: &str,
    _current_account_id: &str,
) -> Result<IndexMap<&'static str, (Vec<String>, Vec<String>)>> {
    let mut cross_service_perms = IndexMap::new();

    // Lambda functions need ECR permissions to pull container images
    if matches!(permission_level, PermissionLevel::Management | PermissionLevel::Provision) {
        let ecr_actions = vec![
            "ecr:BatchGetImage".to_string(),
            "ecr:GetDownloadUrlForLayer".to_string(),
        ];

        let ecr_arns = vec![
            format!("arn:aws:ecr:*:{}:repository/*", managing_account_id),
        ];

        cross_service_perms.insert("ecr", (ecr_actions, ecr_arns));
    }

    Ok(cross_service_perms)
}
```

Now we don't need this anymore because we can define a permission set called `function/execute` that includes ECR access, with access to the management account ID as a variable.

# `alien-permissions` API

alien-permissions core functionallity is to take a PermissionSet and where to bind it, and produce cloud-specific permissions.

Given the permission set from above this is what different strategies produce:

## aws-runtime

This generator is used to create policy documents in AWS that can be created in runtime.

Example for `stack` binding target:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "StorageDataRead",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:GetObjectVersion",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::my-stack-*",
        "arn:aws:s3:::my-stack-*/*"
      ]
    }
  ]
}
```

Example for `resource` binding target:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "StorageDataReadResource",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:GetObjectVersion",
        "s3:ListBucket"
      ],
      "Resource": [
        "arn:aws:s3:::my-stack-payments-data",
        "arn:aws:s3:::my-stack-payments-data/*"
      ]
    }
  ]
}
```

## gcp-runtime

This generator is used to create custom roles in GCP that can be created in runtime.

Custom roles generation is the same for both `runtime` and `stack` targets:

```json
{
  "title": "Storage Data Read",
  "description": "Allows reading data from storage resources",
  "stage": "GA",
  "includedPermissions": [
    "storage.objects.get",
    "storage.objects.list",
    "storage.buckets.get"
  ],
  "name": "projects/my-project/roles/storageDataRead"
}
```

This generator can also generate the `bindings` json often passed to setIamPolicy APIs.

`stack` bindings example (project IAM policy with condition for my-stack- buckets):

```json
{
  "bindings": [
    {
      "role": "projects/my-project/roles/storageDataRead",
      "members": [
        "serviceAccount:my-sa@my-project.iam.gserviceaccount.com"
      ],
      "condition": {
        "title": "Stack-prefixed only",
        "description": "Limit to buckets with prefix my-stack-",
        "expression": "resource.name.startsWith('projects/_/buckets/my-stack-')"
      }
    }
  ]
}
```

`resource` bindings example (applied on the resource):

```json
{
  "bindings": [
    {
      "role": "projects/my-project/roles/storageDataRead",
      "members": [
        "serviceAccount:my-sa@my-project.iam.gserviceaccount.com"
      ]
    }
  ]
}
```

## azure-runtime

This generator is used to create role definitions in Azure that can be created in runtime.

Role definition generation is the same for both `stack` and `resource` targets:

```json
{
  "Name": "Storage Data Read",
  "Id": null,
  "IsCustom": true,
  "Description": "Allows reading data from storage blob containers",
  "Actions": [],
  "NotActions": [],
  "DataActions": [
    "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read"
  ],
  "NotDataActions": [],
  "AssignableScopes": [
    "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod",
    "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod/providers/Microsoft.Storage/storageAccounts/stcxpaymentsprod"
  ]
}
```

Role assignment generation for `stack`:

```json
{
  "properties": {
    "roleDefinitionId": "/subscriptions/00000000-0000-0000-0000-000000000000/providers/Microsoft.Authorization/roleDefinitions/${roleDefinitionGuid}",
    "principalId": "11111111-2222-3333-4444-555555555555",
    "scope": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod"
  }
}
```

Role assignment generation for `resource`:

```json
{
  "properties": {
    "roleDefinitionId": "/subscriptions/00000000-0000-0000-0000-000000000000/providers/Microsoft.Authorization/roleDefinitions/${roleDefinitionGuid}",
    "principalId": "11111111-2222-3333-4444-555555555555",
    "scope": "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-observability-prod/providers/Microsoft.Storage/storageAccounts/stcxpaymentsprod"
  }
}
```

## aws-cloudformation

This generator is used to create CloudFormation templates with IAM resources that can be deployed during stack creation.

Example for `stack` binding target - generates an IAM policy document that can be attached to a role:

```json
{
  "Version": "2012-10-17",
  "Statement": [
    {
      "Sid": "StorageDataRead",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:GetObjectVersion", 
        "s3:ListBucket"
      ],
      "Resource": [
        { "Fn::Sub": "arn:aws:s3:::${AWS::StackName}-*" },
        { "Fn::Sub": "arn:aws:s3:::${AWS::StackName}-*/*" }
      ]
    }
  ]
}
```

Example for `resource` binding target - generates an IAM policy document with resource-specific ARNs:

```json
{
  "Version": "2012-10-17", 
  "Statement": [
    {
      "Sid": "StorageDataReadResource",
      "Effect": "Allow",
      "Action": [
        "s3:GetObject",
        "s3:GetObjectVersion",
        "s3:ListBucket"
      ],
      "Resource": [
        { "Ref": "PaymentsDataBucket" },
        { "Fn::Sub": "${PaymentsDataBucket}/*" }
      ]
    }
  ]
}
```

The key difference from aws-runtime is that this generator uses CloudFormation intrinsic functions like `Fn::Sub`, `Ref`, and `AWS::StackName` to reference resources and parameters that will be resolved during CloudFormation deployment, rather than using literal string values.


### Built-in Permission Sets

Built-in permission sets are stored as JSONC files in `alien-permissions/permission-sets/`, loaded in compile-time:

```
alien-permissions/
├── src/
│   ├── lib.rs
│   ├── engine.rs
│   ├── registry.rs
│   ├── generators/
│   │   ├── aws_runtime.rs
│   │   ├── aws_cloudformation.rs
│   │   ├── gcp.rs
│   │   └── azure.rs
└── permission-sets/
    ├── storage/
    │   ├── data-read.jsonc
    │   ├── data-readwrite.jsonc
    │   ├── management.jsonc
    │   └── provision.jsonc
    ├── function/
    │   ├── execute.jsonc
    │   ├── management.jsonc
    │   ├── provision.jsonc
    │   └── pull-images.jsonc
    └── build/
        ├── execute.jsonc
        └── provision.jsonc
```

^ this is just a sketch and might need more deep thinking.


# Milestones

Milestone 1: Define PermissionSet in `alien-core`. Then, create the `alien-permissions` crate, build the `aws-runtime`, `gcp-runtime`, and `azure-runtime` generators, and test them on some permission sets.

Cases to test:
- `stack` and `resource` binding targets
- some platforms not provided to the permission set
- think of others

design a strategy how to test, maybe even include insta-based snapshot testing.


Milestone 2: Review the old permission system in the worksapce, including all permissions.rs files, and design permission sets that'll be simple and flexible for the user. Start with making a list of all permission sets; then implement them as jsonc files.


Milestone 3: Build the PermissionSet registry in alien-permission. We need to compile the jsonc files into the alien-permissions crate, and have an easy API to get a permissionset from the name.

Milestone 4: Add support for permission profiles in `Stack` in alien-core. 


Milestone 5: Create a new `ServiceAccount` resource (needs in alien-core and alien-infra) and in the alien-infra controllers, use `alien-permissions` to apply permission sets from the profiles. Also Update alien-build to automatically create service accounts from the stack's permission sets.
Note that in AWS we need to add the resource-scoped permissions as well to the role.


Milestone 6: Update AWS/GCP/Azure resource controllers of Build, Artifact Registry, Storage, Function, and infrastructure requirements to apply resource-scoped permissions after the resource is created.

Milestone 7: Build an Azure Lighthouse client in alien-cloud-clients, minimal, only the APIs necessary.

Milestone 8: Remove the old StackManagement and create the RemoteStackManagement resource instead. Add it in stack processor.

Milestone 9: Implement the aws-cloudformation permissions generator and integrate it in alien-infra's template building.


Milestone 10: Delete all the old permission system: alien-infra/permissions/, [resource]/permissions.rs, the `Role` resource, all the old permission providers implemented in alien-infra. 



things to keep in mind that we didn't cover in milestones:

- Variable interpolation and parameterization: Formalize allowed variables (e.g., stackPrefix, resourceName/id, project/subscription, managementAccountId/ExternalId), interpolation rules, and support user-supplied parameters in custom sets.

- Stable naming/versioning: Define deterministic IDs for generated roles/policies: GCP: custom role naming and update semantics. Azure: roleDefinition GUID derivation and update strategy. AWS: policy doc partitioning and role policy naming.

- CLI and DX: Add “permissions plan/explain” in alien-cli (and export artifacts to .alien) to preview resolved profiles, generated policies, and bindings.

Note: we don't care about backwards compatibility. 
