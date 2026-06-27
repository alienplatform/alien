# DataGcp1

## Example Usage

```typescript
import { DataGcp1 } from "@alienplatform/platform-api/models";

let value: DataGcp1 = {
  assignedMachines: 159021,
  capacityGroup: "<value>",
  commandSupported: false,
  daemonInstances: [],
  daemonName: "<value>",
  desiredMachines: 144012,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 314896,
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
  unavailableInstances: 673342,
  backend: "gcp",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `assignedMachines`                                                             | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `capacityGroup`                                                                | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `commandSupported`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `daemonInstances`                                                              | [models.DaemonInstance2](../models/daemoninstance2.md)[]                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `daemonName`                                                                   | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `desiredMachines`                                                              | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent7](../models/syncreconcilerequestevent7.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `healthyInstances`                                                             | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonClusterId`                                                             | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonStatus`                                                                | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonStatusMessage`                                                         | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `horizonStatusReason`                                                          | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `latestUpdateTimestamp`                                                        | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus14](../models/heartbeatstatus14.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `unavailableInstances`                                                         | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"gcp"*                                                                        | :heavy_check_mark:                                                             | N/A                                                                            |