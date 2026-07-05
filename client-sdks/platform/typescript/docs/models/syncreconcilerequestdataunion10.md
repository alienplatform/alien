# SyncReconcileRequestDataUnion10


## Supported Types

### `models.DataAwsIamRole1`

```typescript
const value: models.DataAwsIamRole1 = {
  assumeRolePolicyPresent: false,
  attachedPolicyCount: 410901,
  attachedPolicyNames: [],
  createDate: "<value>",
  inlinePolicyCount: 846965,
  inlinePolicyNames: [],
  managedTagCount: 519428,
  path: "/etc",
  roleArn: "<value>",
  roleId: "<id>",
  roleName: "<value>",
  stackPermissionsApplied: false,
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  tagCount: 250500,
  backend: "awsIamRole",
};
```

### `models.DataGcpServiceAccount1`

```typescript
const value: models.DataGcpServiceAccount1 = {
  email: "Narciso53@hotmail.com",
  projectBindingCount: 780560,
  projectRoles: [
    "<value 1>",
    "<value 2>",
  ],
  serviceAccountBindingCount: 884958,
  serviceAccountRoles: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

### `models.DataAzureManagedIdentity1`

```typescript
const value: models.DataAzureManagedIdentity1 = {
  customRoleDefinitionCount: 863031,
  customRoleDefinitionIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  location: "<value>",
  managedTagCount: 765674,
  name: "<value>",
  resourceGroup: "<value>",
  resourceId: "<id>",
  roleAssignmentCount: 833585,
  roleAssignmentIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  stackPermissionsApplied: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

### `models.DataLocal10`

```typescript
const value: models.DataLocal10 = {
  configured: true,
  identity: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

