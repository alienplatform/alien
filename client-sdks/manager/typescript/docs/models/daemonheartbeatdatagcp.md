# DaemonHeartbeatDataGcp

## Example Usage

```typescript
import { DaemonHeartbeatDataGcp } from "@alienplatform/manager-api/models";

let value: DaemonHeartbeatDataGcp = {
  assignedMachines: 629305,
  capacityGroup: "<value>",
  commandSupported: false,
  daemonName: "<value>",
  desiredMachines: 976582,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  healthyInstances: 396125,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [],
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
| `daemonName`                                                                     | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `desiredMachines`                                                                | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `events`                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `healthyInstances`                                                               | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `horizonClusterId`                                                               | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `horizonStatus`                                                                  | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `horizonStatusMessage`                                                           | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `horizonStatusReason`                                                            | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `instances`                                                                      | [models.HorizonDaemonInstanceStatus](../models/horizondaemoninstancestatus.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `latestUpdateTimestamp`                                                          | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `status`                                                                         | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)           | :heavy_check_mark:                                                               | N/A                                                                              |
| `unavailableInstances`                                                           | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"gcp"*                                                                          | :heavy_check_mark:                                                               | N/A                                                                              |