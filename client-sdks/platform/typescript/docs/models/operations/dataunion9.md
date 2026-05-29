# DataUnion9


## Supported Types

### `operations.DataAwsIamRole1`

```typescript
const value: operations.DataAwsIamRole1 = {
  assumeRolePolicyPresent: false,
  attachedPolicyCount: 410901,
  attachedPolicyNames: [],
  createDate: "<value>",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-06-21T12:26:29.993Z"),
      severity: "warning",
    },
  ],
  inlinePolicyCount: 87215,
  inlinePolicyNames: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  managedTagCount: 456119,
  path: "/media",
  roleArn: "<value>",
  roleId: "<id>",
  roleName: "<value>",
  stackPermissionsApplied: false,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "updating",
    partial: false,
    stale: true,
  },
  tagCount: 813150,
  backend: "awsIamRole",
};
```

### `operations.DataGcpServiceAccount1`

```typescript
const value: operations.DataGcpServiceAccount1 = {
  email: "Narciso53@hotmail.com",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-11-02T07:18:24.714Z"),
      severity: "error",
    },
  ],
  projectBindingCount: 974149,
  projectRoles: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  serviceAccountBindingCount: 694359,
  serviceAccountRoles: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "failed",
    partial: true,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

### `operations.DataAzureManagedIdentity1`

```typescript
const value: operations.DataAzureManagedIdentity1 = {
  customRoleDefinitionCount: 863031,
  customRoleDefinitionIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-07-02T14:37:18.556Z"),
      severity: "error",
    },
  ],
  location: "<value>",
  managedTagCount: 626517,
  name: "<value>",
  resourceGroup: "<value>",
  resourceId: "<id>",
  roleAssignmentCount: 780094,
  roleAssignmentIds: [
    "<value 1>",
  ],
  stackPermissionsApplied: false,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  backend: "azureManagedIdentity",
};
```

### `operations.DataLocal9`

```typescript
const value: operations.DataLocal9 = {
  configured: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-12-06T21:51:19.391Z"),
      severity: "error",
    },
  ],
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
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  backend: "local",
};
```

