# DataDaemon

## Example Usage

```typescript
import { DataDaemon } from "@alienplatform/platform-api/models";

let value: DataDaemon = {
  data: {
    assignedMachines: 489905,
    capacityGroup: "<value>",
    commandSupported: true,
    daemonInstances: [],
    daemonName: "<value>",
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
          reason: "api-unavailable",
          severity: "warning",
          source: "<value>",
        },
      ],
      health: "unhealthy",
      lifecycle: "running",
      partial: false,
      stale: true,
    },
    unavailableInstances: 431179,
    backend: "gcp",
  },
  resourceType: "daemon",
};
```

## Fields

| Field                                   | Type                                    | Required                                | Description                             |
| --------------------------------------- | --------------------------------------- | --------------------------------------- | --------------------------------------- |
| `data`                                  | *models.SyncReconcileRequestDataUnion4* | :heavy_check_mark:                      | N/A                                     |
| `resourceType`                          | *"daemon"*                              | :heavy_check_mark:                      | N/A                                     |