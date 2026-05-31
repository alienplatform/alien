# ServiceAccountHeartbeatData


## Supported Types

### `models.ServiceAccountHeartbeatDataAwsIamRole`

```typescript
const value: models.ServiceAccountHeartbeatDataAwsIamRole = {
  assumeRolePolicyPresent: true,
  attachedPolicyCount: 703796,
  attachedPolicyNames: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  createDate: "<value>",
  inlinePolicyCount: 840219,
  inlinePolicyNames: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  managedTagCount: 681671,
  path: "/usr/share",
  roleArn: "<value>",
  roleId: "<id>",
  roleName: "<value>",
  stackPermissionsApplied: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  tagCount: 317385,
  backend: "awsIamRole",
};
```

### `models.ServiceAccountHeartbeatDataGcpServiceAccount`

```typescript
const value: models.ServiceAccountHeartbeatDataGcpServiceAccount = {
  email: "Kraig_Jast-Koss80@yahoo.com",
  projectBindingCount: 864516,
  projectRoles: [
    "<value 1>",
    "<value 2>",
  ],
  serviceAccountBindingCount: 255611,
  serviceAccountRoles: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

### `models.ServiceAccountHeartbeatDataAzureManagedIdentity`

```typescript
const value: models.ServiceAccountHeartbeatDataAzureManagedIdentity = {
  customRoleDefinitionCount: 128295,
  customRoleDefinitionIds: [
    "<value 1>",
    "<value 2>",
  ],
  location: "<value>",
  managedTagCount: 375916,
  name: "<value>",
  resourceGroup: "<value>",
  resourceId: "<id>",
  roleAssignmentCount: 826820,
  roleAssignmentIds: [
    "<value 1>",
  ],
  stackPermissionsApplied: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "azureManagedIdentity",
};
```

### `models.ServiceAccountHeartbeatDataLocal`

```typescript
const value: models.ServiceAccountHeartbeatDataLocal = {
  configured: false,
  identity: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: false,
    stale: true,
  },
  backend: "local",
};
```

