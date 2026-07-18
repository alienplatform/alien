# DaemonHeartbeatDataAws

## Example Usage

```typescript
import { DaemonHeartbeatDataAws } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataAws = {
  assignedMachines: 818243,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 915980,
  events: [],
  healthyInstances: 401388,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  unavailableInstances: 429918,
  backend: "aws",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `assignedMachines`                                                               | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `capacityGroup`                                                                  | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `commandSupported`                                                               | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `daemonInstances`                                                                | [models.ManagedRuntimeUnitStatus](../models/managedruntimeunitstatus.md)[]       | :heavy_check_mark:                                                               | N/A                                                                              |
| `daemonName`                                                                     | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `desiredMachines`                                                                | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `events`                                                                         | [models.ManagedRuntimeEventSnapshot](../models/managedruntimeeventsnapshot.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `healthyInstances`                                                               | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `horizonClusterId`                                                               | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `horizonStatus`                                                                  | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `horizonStatusMessage`                                                           | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `horizonStatusReason`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `latestUpdateTimestamp`                                                          | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `observedImage`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)           | :heavy_check_mark:                                                               | N/A                                                                              |
| `unavailableInstances`                                                           | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"aws"*                                                                          | :heavy_check_mark:                                                               | N/A                                                                              |