# DataUnion11


## Supported Types

### `operations.DataAwsIamRole2`

```typescript
const value: operations.DataAwsIamRole2 = {
  events: [],
  managementPermissionsApplied: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: true,
    stale: false,
  },
  backend: "awsIamRole",
};
```

### `operations.DataGcpServiceAccount2`

```typescript
const value: operations.DataGcpServiceAccount2 = {
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
        reason: "api-unavailable",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "gcpServiceAccount",
};
```

### `operations.DataAzureManagedIdentity2`

```typescript
const value: operations.DataAzureManagedIdentity2 = {
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

