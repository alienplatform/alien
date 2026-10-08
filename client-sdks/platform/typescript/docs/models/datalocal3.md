# DataLocal3

## Example Usage

```typescript
import { DataLocal3 } from "@alienplatform/platform-api/models";

let value: DataLocal3 = {
  bindMountCount: 241047,
  events: [],
  portCount: 395842,
  runtimeReachable: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "stopping",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `bindMountCount`                                                               | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `containerId`                                                                  | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `containerUnit`                                                                | *models.ContainerUnitUnion*                                                    | :heavy_minus_sign:                                                             | N/A                                                                            |
| `cpu`                                                                          | *models.CpuUnion5*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent5](../models/syncreconcilerequestevent5.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `image`                                                                        | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `localUrl`                                                                     | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion5*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `name`                                                                         | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `portCount`                                                                    | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `restartCount`                                                                 | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `runtimeReachable`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `runtimeStatus`                                                                | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.ResourceHeartbeatStatus12](../models/resourceheartbeatstatus12.md)     | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"local"*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |