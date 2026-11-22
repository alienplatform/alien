# DataDaemon

## Example Usage

```typescript
import { DataDaemon } from "@alienplatform/platform-api/models/operations";

let value: DataDaemon = {
  data: {
    assignedMachines: 489905,
    capacityGroup: "<value>",
    commandSupported: true,
    daemonInstances: [],
    desiredMachines: 665477,
    events: [
      {
        message: "<value>",
        reason: "<value>",
      },
    ],
    healthyInstances: 921353,
    horizonClusterId: "<id>",
    horizonStatus: "<value>",
    latestUpdateTimestamp: "<value>",
    status: {
      collectionIssues: [
        {
          message: "<value>",
          reason: "not-installed",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unknown",
      lifecycle: "running",
      partial: false,
      stale: false,
    },
    unavailableInstances: 431179,
    backend: "azure",
  },
  resourceType: "daemon",
};
```

## Fields

| Field                   | Type                    | Required                | Description             |
| ----------------------- | ----------------------- | ----------------------- | ----------------------- |
| `data`                  | *operations.DataUnion4* | :heavy_check_mark:      | N/A                     |
| `resourceType`          | *"daemon"*              | :heavy_check_mark:      | N/A                     |