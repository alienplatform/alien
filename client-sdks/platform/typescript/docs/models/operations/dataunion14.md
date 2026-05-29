# DataUnion14


## Supported Types

### `operations.DataGcpServiceUsage`

```typescript
const value: operations.DataGcpServiceUsage = {
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
        severity: "info",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "stopping",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceUsage",
};
```

### `operations.DataAzureResourceProvider`

```typescript
const value: operations.DataAzureResourceProvider = {
  events: [],
  namespace: "<value>",
  registered: false,
  resourceTypeCount: 249113,
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
    lifecycle: "running",
    partial: false,
    stale: false,
  },
  backend: "azureResourceProvider",
};
```

