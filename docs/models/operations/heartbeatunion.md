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
        imageIdentifier: "<value>",
        status: {
          collectionIssues: [
            {
              message: "<value>",
              reason: "timed-out",
              severity: "info",
              source: "<value>",
            },
          ],
          health: "healthy",
          lifecycle: "updating",
          partial: true,
          stale: true,
        },
        backend: "awsMicrovm",
      },
      resourceType: "sandbox",
    },
    observedAt: new Date("2024-04-26T23:19:48.455Z"),
    raw: [
      {
        body: "<value>",
        collectedAt: new Date("2025-01-29T23:35:36.058Z"),
        format: "json",
        source: "<value>",
        truncated: true,
      },
    ],
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

