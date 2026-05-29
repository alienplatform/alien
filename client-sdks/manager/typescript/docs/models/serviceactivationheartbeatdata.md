# ServiceActivationHeartbeatData


## Supported Types

### `models.ServiceActivationHeartbeatDataGcpServiceUsage`

```typescript
const value: models.ServiceActivationHeartbeatDataGcpServiceUsage = {
  enabled: false,
  projectId: "<id>",
  serviceName: "<value>",
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "gcpServiceUsage",
};
```

### `models.ServiceActivationHeartbeatDataAzureResourceProvider`

```typescript
const value: models.ServiceActivationHeartbeatDataAzureResourceProvider = {
  namespace: "<value>",
  registered: true,
  resourceTypeCount: 15021,
  status: {
    collectionIssues: [],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: true,
  },
  backend: "azureResourceProvider",
};
```

