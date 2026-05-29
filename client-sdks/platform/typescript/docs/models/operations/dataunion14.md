# DataUnion14


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

