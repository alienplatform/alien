# RemoteStackManagementHeartbeatData


## Supported Types

### `models.RemoteStackManagementHeartbeatDataAwsIamRole`

```typescript
const value: models.RemoteStackManagementHeartbeatDataAwsIamRole = {
  events: [],
  managementPermissionsApplied: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "awsIamRole",
};
```

### `models.RemoteStackManagementHeartbeatDataGcpServiceAccount`

```typescript
const value: models.RemoteStackManagementHeartbeatDataGcpServiceAccount = {
  events: [],
  impersonationGranted: true,
  roleBound: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceAccount",
};
```

### `models.RemoteStackManagementHeartbeatDataAzureManagedIdentity`

```typescript
const value: models.RemoteStackManagementHeartbeatDataAzureManagedIdentity = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  roleAssignmentIds: [
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
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

