# SyncReconcileRequestDataUnion11


## Supported Types

### `models.DataAwsIamRole2`

```typescript
const value: models.DataAwsIamRole2 = {
  events: [],
  managementPermissionsApplied: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "awsIamRole",
};
```

### `models.DataGcpServiceAccount2`

```typescript
const value: models.DataGcpServiceAccount2 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-09-28T13:47:01.596Z"),
      severity: "error",
    },
  ],
  impersonationGranted: true,
  roleBound: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceAccount",
};
```

### `models.DataAzureManagedIdentity2`

```typescript
const value: models.DataAzureManagedIdentity2 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-06-15T03:37:32.834Z"),
      severity: "info",
    },
  ],
  roleAssignmentIds: [],
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "updating",
    partial: false,
    stale: true,
  },
  backend: "azureManagedIdentity",
};
```

