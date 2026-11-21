# DataUnion16


## Supported Types

### `operations.DataGcpServiceUsage`

```typescript
const value: operations.DataGcpServiceUsage = {
  enabled: true,
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "warning",
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

### `operations.DataAzureResourceProvider`

```typescript
const value: operations.DataAzureResourceProvider = {
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
