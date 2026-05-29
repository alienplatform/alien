# DataDaemon

## Example Usage

```typescript
import { DataDaemon } from "@alienplatform/platform-api/models/operations";

let value: DataDaemon = {
  data: {
    assignedMachines: 489905,
    capacityGroup: "<value>",
    commandSupported: true,
    daemonName: "<value>",
    desiredMachines: 300440,
    events: [
      {
        kind: "<value>",
        message: "<value>",
        observedAt: new Date("2025-12-12T10:53:03.618Z"),
        severity: "error",
      },
    ],
    healthyInstances: 431179,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    instances: [
      {
        name: "<value>",
        ready: true,
        replicaId: "<id>",
      },
    ],
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
    unavailableInstances: 833736,
    backend: "gcp",
  },
  resourceType: "daemon",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion4* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"daemon"*              | :heavy_check_mark:      | N/A                     |