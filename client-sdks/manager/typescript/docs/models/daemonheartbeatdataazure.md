# DaemonHeartbeatDataAzure

## Example Usage

```typescript
import { DaemonHeartbeatDataAzure } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataAzure = {
  assignedMachines: 950576,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonInstances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  desiredMachines: 469652,
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  healthyInstances: 926835,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
  unavailableInstances: 821001,
  backend: "azure",
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
| `status`                                                                         | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)           | :heavy_check_mark:                                                               | N/A                                                                              |
| `unavailableInstances`                                                           | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"azure"*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |