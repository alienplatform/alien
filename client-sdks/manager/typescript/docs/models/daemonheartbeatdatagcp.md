# DaemonHeartbeatDataGcp

## Example Usage

```typescript
import { DaemonHeartbeatDataGcp } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataGcp = {
  assignedMachines: 629305,
  capacityGroup: "<value>",
  commandSupported: false,
  daemonInstances: [
    {
      name: "<value>",
      ready: true,
      replicaId: "<id>",
    },
  ],
  daemonName: "<value>",
  desiredMachines: 550789,
  events: [],
  healthyInstances: 289674,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  unavailableInstances: 685429,
  backend: "gcp",
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `assignedMachines`                                                               | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `capacityGroup`                                                                  | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `commandSupported`                                                               | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `daemonInstances`                                                                | [models.ManagedRuntimeUnitStatus](../models/managedruntimeunitstatus.md)[]       | :heavy_check_mark:                                                               | N/A                                                                              |
| `daemonName`                                                                     | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
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
| `backend`                                                                        | *"gcp"*                                                                          | :heavy_check_mark:                                                               | N/A                                                                              |