# SyncReconcileRequestDataUnion15


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
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: true,
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

