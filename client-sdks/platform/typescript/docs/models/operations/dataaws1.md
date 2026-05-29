# DataAws1

## Example Usage

```typescript
import { DataAws1 } from "@alienplatform/platform-api/models/operations";

let value: DataAws1 = {
  assignedMachines: 644340,
  capacityGroup: "<value>",
  commandSupported: true,
  daemonName: "<value>",
  desiredMachines: 753830,
  events: [],
  healthyInstances: 896332,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [
    {
      name: "<value>",
      ready: false,
      replicaId: "<id>",
    },
  ],
  latestUpdateTimestamp: "<value>",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "deleted",
    partial: false,
    stale: false,
  },
  unavailableInstances: 873077,
  backend: "aws",
};
```

## Fields

| Field                                                                                                        | Type                                                                                                         | Required                                                                                                     | Description                                                                                                  |
| ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------ |
| `assignedMachines`                                                                                           | *number*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `capacityGroup`                                                                                              | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `commandSupported`                                                                                           | *boolean*                                                                                                    | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `daemonName`                                                                                                 | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `desiredMachines`                                                                                            | *number*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `events`                                                                                                     | [operations.GetRawResourceHeartbeatEvent13](../../models/operations/getrawresourceheartbeatevent13.md)[]     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `healthyInstances`                                                                                           | *number*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `horizonClusterId`                                                                                           | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `horizonStatus`                                                                                              | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `horizonStatusMessage`                                                                                       | *string*                                                                                                     | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `horizonStatusReason`                                                                                        | *string*                                                                                                     | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `instances`                                                                                                  | [operations.GetRawResourceHeartbeatInstance3](../../models/operations/getrawresourceheartbeatinstance3.md)[] | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `latestUpdateTimestamp`                                                                                      | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `status`                                                                                                     | [operations.DataStatus13](../../models/operations/datastatus13.md)                                           | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `unavailableInstances`                                                                                       | *number*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `backend`                                                                                                    | *"aws"*                                                                                                      | :heavy_check_mark:                                                                                           | N/A                                                                                                          |