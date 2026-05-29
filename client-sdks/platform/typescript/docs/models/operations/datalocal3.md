# DataLocal3

## Example Usage

```typescript
import { DataLocal3 } from "@alienplatform/platform-api/models/operations";

let value: DataLocal3 = {
  bindMountCount: 241047,
  events: [],
  portCount: 395842,
  runtimeReachable: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "updating",
    partial: true,
    stale: true,
  },
  backend: "local",
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `bindMountCount`                                                                                         | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `containerId`                                                                                            | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `cpu`                                                                                                    | *operations.CpuUnion5*                                                                                   | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent12](../../models/operations/getrawresourceheartbeatevent12.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `image`                                                                                                  | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `localUrl`                                                                                               | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `memory`                                                                                                 | *operations.MemoryUnion5*                                                                                | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `portCount`                                                                                              | *number*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `restartCount`                                                                                           | *number*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `runtimeReachable`                                                                                       | *boolean*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `runtimeStatus`                                                                                          | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus12](../../models/operations/datastatus12.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `backend`                                                                                                | *"local"*                                                                                                | :heavy_check_mark:                                                                                       | N/A                                                                                                      |