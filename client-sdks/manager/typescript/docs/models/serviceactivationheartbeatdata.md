# ServiceActivationHeartbeatData


## Supported Types

### `models.ServiceActivationHeartbeatDataGcpServiceUsage`

```typescript
const value: models.ServiceActivationHeartbeatDataGcpServiceUsage = {
  enabled: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: true,
    stale: false,
  },
  backend: "gcpServiceUsage",
};
```

### `models.ServiceActivationHeartbeatDataAzureResourceProvider`

```typescript
const value: models.ServiceActivationHeartbeatDataAzureResourceProvider = {
  events: [],
  namespace: "<value>",
  registered: true,
  resourceTypeCount: 306090,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: true,
    stale: false,
  },
  backend: "azureResourceProvider",
};
```

