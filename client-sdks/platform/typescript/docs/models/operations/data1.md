# Data1

## Example Usage

```typescript
import { Data1 } from "@alienplatform/platform-api/models/operations";

let value: Data1 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-08-02T12:07:39.617Z"),
      severity: "error",
    },
  ],
  name: "<value>",
  nodeCounts: {},
  podCounts: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: false,
    stale: false,
  },
};
```

## Fields

| Field                                                                                                    | Type                                                                                                     | Required                                                                                                 | Description                                                                                              |
| -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| `cpu`                                                                                                    | *operations.CpuUnion10*                                                                                  | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `events`                                                                                                 | [operations.GetRawResourceHeartbeatEvent22](../../models/operations/getrawresourceheartbeatevent22.md)[] | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `memory`                                                                                                 | *operations.MemoryUnion10*                                                                               | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `name`                                                                                                   | *string*                                                                                                 | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `namespace`                                                                                              | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `nodeCounts`                                                                                             | [operations.NodeCounts](../../models/operations/nodecounts.md)                                           | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `nodeStatuses`                                                                                           | [operations.NodeStatus](../../models/operations/nodestatus.md)[]                                         | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `podCounts`                                                                                              | [operations.PodCounts](../../models/operations/podcounts.md)                                             | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `region`                                                                                                 | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |
| `status`                                                                                                 | [operations.DataStatus22](../../models/operations/datastatus22.md)                                       | :heavy_check_mark:                                                                                       | N/A                                                                                                      |
| `version`                                                                                                | *string*                                                                                                 | :heavy_minus_sign:                                                                                       | N/A                                                                                                      |