# DataMachines1

## Example Usage

```typescript
import { DataMachines1 } from "@alienplatform/platform-api/models";

let value: DataMachines1 = {
  assignedMachines: 323362,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 395294,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 133817,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
  unavailableInstances: 26745,
  backend: "machines",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `assignedMachines`                                                             | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `capacityGroup`                                                                | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `commandSupported`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `daemonInstances`                                                              | [models.DaemonInstance4](../models/daemoninstance4.md)[]                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `daemonName`                                                                   | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `desiredMachines`                                                              | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent9](../models/syncreconcilerequestevent9.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `healthyInstances`                                                             | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonClusterId`                                                             | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonStatus`                                                                | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonStatusMessage`                                                         | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `horizonStatusReason`                                                          | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `latestUpdateTimestamp`                                                        | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `observedImage`                                                                | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.ResourceHeartbeatStatus16](../models/resourceheartbeatstatus16.md)     | :heavy_check_mark:                                                             | N/A                                                                            |
| `unavailableInstances`                                                         | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"machines"*                                                                   | :heavy_check_mark:                                                             | N/A                                                                            |