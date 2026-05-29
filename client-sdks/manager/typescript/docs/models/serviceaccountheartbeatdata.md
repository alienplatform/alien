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
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  inlinePolicyCount: 792146,
  inlinePolicyNames: [
    "<value 1>",
    "<value 2>",
  ],
  managedTagCount: 874935,
  path: "/var/log",
  roleArn: "<value>",
  roleId: "<id>",
  roleName: "<value>",
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
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  tagCount: 961527,
  backend: "awsIamRole",
};
```

### `models.ServiceAccountHeartbeatDataGcpServiceAccount`

```typescript
const value: models.ServiceAccountHeartbeatDataGcpServiceAccount = {
  email: "Kraig_Jast-Koss80@yahoo.com",
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  projectBindingCount: 541422,
  projectRoles: [
    "<value 1>",
  ],
  serviceAccountBindingCount: 635399,
  serviceAccountRoles: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
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
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
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
  events: [],
  location: "<value>",
  managedTagCount: 826820,
  name: "<value>",
  resourceGroup: "<value>",
  resourceId: "<id>",
  roleAssignmentCount: 359064,
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
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

### `models.ServiceAccountHeartbeatDataLocal`

```typescript
const value: models.ServiceAccountHeartbeatDataLocal = {
  configured: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
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
    health: "degraded",
    lifecycle: "deleting",
    partial: true,
    stale: false,
  },
  backend: "local",
};
```

