# HeartbeatUnion


## Supported Types

### `operations.HeartbeatAvailable`

```typescript
const value: operations.HeartbeatAvailable = {
  status: "available",
  deploymentId: "<id>",
  resourceId: "<id>",
  resourceType: "<value>",
  backend: "<value>",
  controllerPlatform: "<value>",
  observedAt: new Date("2024-06-21T00:18:54.209Z"),
  staleAt: new Date("2024-03-07T10:23:33.467Z"),
  platformStale: false,
  heartbeat: {
    backend: "azure",
    controllerPlatform: "gcp",
    data: {
      data: {
        name: "<value>",
        privateEndpointConnectionCount: 152029,
        status: {
          collectionIssues: [
            {
              message: "<value>",
              reason: "not-installed",
              severity: "info",
              source: "<value>",
            },
          ],
          health: "unknown",
          lifecycle: "running",
          partial: false,
          stale: true,
        },
      },
      resourceType: "azure_service_bus_namespace",
    },
    observedAt: new Date("2024-03-08T08:46:36.237Z"),
    raw: [],
    resourceId: "<id>",
    resourceType: "<value>",
  },
  raw: [
    "<value 1>",
    "<value 2>",
    "<value 3>",
  ],
};
```

### `operations.HeartbeatMissing`

```typescript
const value: operations.HeartbeatMissing = {
  status: "missing",
  deploymentId: "<id>",
  resourceId: "<id>",
  resourceType: "<value>",
};
```

