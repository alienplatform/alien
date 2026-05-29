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
        reason: "api-unavailable",
        severity: "warning",
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

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `bindMountCount`                                                                 | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `containerId`                                                                    | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `cpu`                                                                            | *models.CpuUnion5*                                                               | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent12](../models/syncreconcilerequestevent12.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `image`                                                                          | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `localUrl`                                                                       | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion5*                                                            | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `portCount`                                                                      | *number*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `restartCount`                                                                   | *number*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `runtimeReachable`                                                               | *boolean*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |
| `runtimeStatus`                                                                  | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus12](../models/heartbeatstatus12.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `backend`                                                                        | *"local"*                                                                        | :heavy_check_mark:                                                               | N/A                                                                              |