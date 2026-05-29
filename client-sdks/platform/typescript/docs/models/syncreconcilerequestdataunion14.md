# SyncReconcileRequestDataUnion14


## Supported Types

### `models.DataGcpServiceUsage`

```typescript
const value: models.DataGcpServiceUsage = {
  enabled: true,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2025-06-11T19:58:11.111Z"),
      severity: "error",
    },
  ],
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceUsage",
};
```

### `models.DataAzureResourceProvider`

```typescript
const value: models.DataAzureResourceProvider = {
  events: [],
  namespace: "<value>",
  registered: false,
  resourceTypeCount: 249113,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: true,
    stale: true,
  },
  backend: "azureResourceProvider",
};
```

