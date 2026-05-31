# DataAws1

## Example Usage

```typescript
import { DataAws1 } from "@alienplatform/platform-api/models";

let value: DataAws1 = {
  assignedMachines: 644340,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: true,
      replicaId: "<id>",
    },
  ],
  daemonName: "<value>",
  desiredMachines: 896332,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 510224,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "unknown",
    partial: false,
    stale: false,
  },
  unavailableInstances: 702316,
  backend: "aws",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `assignedMachines`                                                             | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `capacityGroup`                                                                | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `commandSupported`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `daemonInstances`                                                              | [models.DaemonInstance1](../models/daemoninstance1.md)[]                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `daemonName`                                                                   | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `desiredMachines`                                                              | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent6](../models/syncreconcilerequestevent6.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `healthyInstances`                                                             | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonClusterId`                                                             | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonStatus`                                                                | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `horizonStatusMessage`                                                         | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `horizonStatusReason`                                                          | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `latestUpdateTimestamp`                                                        | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus13](../models/heartbeatstatus13.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `unavailableInstances`                                                         | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"aws"*                                                                        | :heavy_check_mark:                                                             | N/A                                                                            |