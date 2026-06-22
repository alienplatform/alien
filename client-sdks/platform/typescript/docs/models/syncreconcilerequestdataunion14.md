# SyncReconcileRequestDataUnion14


## Supported Types

### `models.DataGcpServiceUsage`

```typescript
const value: models.DataGcpServiceUsage = {
  enabled: true,
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "deleted",
    partial: false,
    stale: false,
  },
  backend: "gcpServiceUsage",
};
```

### `models.DataAzureResourceProvider`

```typescript
const value: models.DataAzureResourceProvider = {
  namespace: "<value>",
  registered: true,
  resourceTypeCount: 563831,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "scaling",
    partial: true,
    stale: true,
  },
  backend: "azureResourceProvider",
};
```

