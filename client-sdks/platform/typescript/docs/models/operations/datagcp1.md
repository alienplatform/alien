# DataGcp1

## Example Usage

```typescript
import { DataGcp1 } from "@alienplatform/platform-api/models/operations";

let value: DataGcp1 = {
  assignedMachines: 159021,
  capacityGroup: "<value>",
  commandSupported: false,
  daemonName: "<value>",
  desiredMachines: 139194,
  events: [],
  healthyInstances: 504997,
  horizonClusterId: "<id>",
  horizonStatus: "<value>",
  instances: [],
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
    health: "degraded",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  unavailableInstances: 530018,
  backend: "gcp",
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
| `events`                                                                                                     | [operations.GetRawResourceHeartbeatEvent14](../../models/operations/getrawresourceheartbeatevent14.md)[]     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `healthyInstances`                                                                                           | *number*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `horizonClusterId`                                                                                           | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `horizonStatus`                                                                                              | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `horizonStatusMessage`                                                                                       | *string*                                                                                                     | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `horizonStatusReason`                                                                                        | *string*                                                                                                     | :heavy_minus_sign:                                                                                           | N/A                                                                                                          |
| `instances`                                                                                                  | [operations.GetRawResourceHeartbeatInstance4](../../models/operations/getrawresourceheartbeatinstance4.md)[] | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `latestUpdateTimestamp`                                                                                      | *string*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `status`                                                                                                     | [operations.DataStatus14](../../models/operations/datastatus14.md)                                           | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `unavailableInstances`                                                                                       | *number*                                                                                                     | :heavy_check_mark:                                                                                           | N/A                                                                                                          |
| `backend`                                                                                                    | *"gcp"*                                                                                                      | :heavy_check_mark:                                                                                           | N/A                                                                                                          |