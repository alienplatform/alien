# SyncReconcileRequestDataUnion12


## Supported Types

### `models.DataAwsIamRole2`

```typescript
const value: models.DataAwsIamRole2 = {
  managementPermissionsApplied: true,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "awsIamRole",
};
```

### `models.DataGcpServiceAccount2`

```typescript
const value: models.DataGcpServiceAccount2 = {
  impersonationGranted: false,
  roleBound: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "gcpServiceAccount",
};
```

### `models.DataAzureManagedIdentity2`

```typescript
const value: models.DataAzureManagedIdentity2 = {
  roleAssignmentIds: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
  status: {
    collectionIssues: [],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "azureManagedIdentity",
};
```

