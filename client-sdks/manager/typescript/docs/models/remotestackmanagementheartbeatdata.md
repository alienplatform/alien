# RemoteStackManagementHeartbeatData


## Supported Types

### `models.RemoteStackManagementHeartbeatDataAwsIamRole`

```typescript
const value: models.RemoteStackManagementHeartbeatDataAwsIamRole = {
  managementPermissionsApplied: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "awsIamRole",
};
```

### `models.RemoteStackManagementHeartbeatDataGcpServiceAccount`

```typescript
const value: models.RemoteStackManagementHeartbeatDataGcpServiceAccount = {
  impersonationGranted: true,
  roleBound: true,
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

### `models.RemoteStackManagementHeartbeatDataAzureManagedIdentity`

```typescript
const value: models.RemoteStackManagementHeartbeatDataAzureManagedIdentity = {
  roleAssignmentIds: [
    "<value 1>",
    "<value 2>",
  ],
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "running",
    partial: true,
    stale: true,
  },
  backend: "azureManagedIdentity",
};
```

