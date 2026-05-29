# GetRawResourceHeartbeatResponse

Latest raw typed resource heartbeat for a deployment resource.


## Supported Types

### `operations.Available`

```typescript
const value: operations.Available = {
  status: "available",
  deploymentId: "<id>",
  resourceId: "<id>",
  resourceType: "<value>",
  backend: "<value>",
  controllerPlatform: "<value>",
  observedAt: new Date("2025-07-05T10:41:00.131Z"),
  staleAt: new Date("2025-05-06T14:56:57.163Z"),
  platformStale: false,
  heartbeat: {
    backend: "managed",
    controllerPlatform: "gcp",
    data: {
      data: {
        assignedMachines: 411840,
        capacityGroup: "<value>",
        commandSupported: true,
        daemonName: "<value>",
        desiredMachines: 184409,
        events: [
          {
            kind: "<value>",
            message: "<value>",
            observedAt: new Date("2025-12-12T10:53:03.618Z"),
            severity: "error",
          },
        ],
        healthyInstances: 167419,
        horizonClusterId: "<id>",
        horizonStatus: "<value>",
        instances: [],
        latestUpdateTimestamp: "<value>",
        status: {
          collectionIssues: [
            {
              message: "<value>",
              reason: "collection-failed",
              severity: "info",
              source: "<value>",
            },
          ],
          health: "degraded",
          lifecycle: "running",
          partial: false,
          stale: true,
        },
        unavailableInstances: 645411,
        backend: "gcp",
      },
      resourceType: "daemon",
    },
    observedAt: new Date("2026-11-29T14:37:16.114Z"),
    raw: [],
    resourceId: "<id>",
    resourceType: "<value>",
  },
  raw: [
    "<value 1>",
    "<value 2>",
  ],
};
```

### `operations.Missing`

```typescript
const value: operations.Missing = {
  status: "missing",
  deploymentId: "<id>",
  resourceId: "<id>",
  resourceType: "<value>",
};
```

