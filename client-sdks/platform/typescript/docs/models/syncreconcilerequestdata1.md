# SyncReconcileRequestData1

## Example Usage

```typescript
import { SyncReconcileRequestData1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData1 = {
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
    collectionIssues: [
      {
        message: "<value>",
        reason: "timed-out",
        severity: "warning",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "stopping",
    partial: true,
    stale: false,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cpu`                                                                            | *models.CpuUnion11*                                                              | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent12](../models/syncreconcilerequestevent12.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion11*                                                           | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `nodeCounts`                                                                     | [models.NodeCounts](../models/nodecounts.md)                                     | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodeStatuses`                                                                   | [models.NodeStatus](../models/nodestatus.md)[]                                   | :heavy_minus_sign:                                                               | N/A                                                                              |
| `podCounts`                                                                      | [models.PodCounts](../models/podcounts.md)                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `region`                                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.ResourceHeartbeatStatus24](../models/resourceheartbeatstatus24.md)       | :heavy_check_mark:                                                               | N/A                                                                              |
| `version`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |