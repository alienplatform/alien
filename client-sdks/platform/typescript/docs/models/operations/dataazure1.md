# DataAzure1

## Example Usage

```typescript
import { DataAzure1 } from "@alienplatform/platform-api/models/operations";

let value: DataAzure1 = {
  assignedMachines: 3703,
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
  desiredMachines: 583805,
  events: [],
  healthyInstances: 986297,
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
  unavailableInstances: 614597,
  backend: "azure",
};
```

## Fields

| Field                                                                                                  | Type                                                                                                   | Required                                                                                               | Description                                                                                            |
| ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| `assignedMachines`                                                                                     | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `capacityGroup`                                                                                        | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `commandSupported`                                                                                     | *boolean*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `daemonInstances`                                                                                      | [operations.DaemonInstance3](../../models/operations/daemoninstance3.md)[]                             | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `daemonName`                                                                                           | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `desiredMachines`                                                                                      | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `events`                                                                                               | [operations.GetRawResourceHeartbeatEvent8](../../models/operations/getrawresourceheartbeatevent8.md)[] | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `healthyInstances`                                                                                     | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `horizonClusterId`                                                                                     | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `horizonStatus`                                                                                        | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `horizonStatusMessage`                                                                                 | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `horizonStatusReason`                                                                                  | *string*                                                                                               | :heavy_minus_sign:                                                                                     | N/A                                                                                                    |
| `latestUpdateTimestamp`                                                                                | *string*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `status`                                                                                               | [operations.DataStatus15](../../models/operations/datastatus15.md)                                     | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `unavailableInstances`                                                                                 | *number*                                                                                               | :heavy_check_mark:                                                                                     | N/A                                                                                                    |
| `backend`                                                                                              | *"azure"*                                                                                              | :heavy_check_mark:                                                                                     | N/A                                                                                                    |