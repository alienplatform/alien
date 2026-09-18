# Data1

## Example Usage

```typescript
import { Data1 } from "@alienplatform/platform-api/models/operations";

let value: Data1 = {
  events: [
    {
      message: "<value>",
      reason: "<value>",
    },
  ],
  name: "<value>",
  nodeCounts: {},
  podCounts: {},
  status: {
    collectionIssues: [],
    health: "unhealthy",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
};
```

## Fields

| Field                                                              | Type                                                               | Required                                                           | Description                                                        |
| ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ | ------------------------------------------------------------------ |
| `cpu`                                                              | *operations.CpuUnion11*                                            | :heavy_minus_sign:                                                 | N/A                                                                |
| `events`                                                           | [operations.Event12](../../models/operations/event12.md)[]         | :heavy_check_mark:                                                 | N/A                                                                |
| `memory`                                                           | *operations.MemoryUnion11*                                         | :heavy_minus_sign:                                                 | N/A                                                                |
| `name`                                                             | *string*                                                           | :heavy_check_mark:                                                 | N/A                                                                |
| `namespace`                                                        | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `nodeCounts`                                                       | [operations.NodeCounts](../../models/operations/nodecounts.md)     | :heavy_check_mark:                                                 | N/A                                                                |
| `nodeStatuses`                                                     | [operations.NodeStatus](../../models/operations/nodestatus.md)[]   | :heavy_minus_sign:                                                 | N/A                                                                |
| `podCounts`                                                        | [operations.PodCounts](../../models/operations/podcounts.md)       | :heavy_check_mark:                                                 | N/A                                                                |
| `region`                                                           | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |
| `status`                                                           | [operations.DataStatus24](../../models/operations/datastatus24.md) | :heavy_check_mark:                                                 | N/A                                                                |
| `version`                                                          | *string*                                                           | :heavy_minus_sign:                                                 | N/A                                                                |