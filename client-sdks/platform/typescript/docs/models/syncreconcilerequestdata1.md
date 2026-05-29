# SyncReconcileRequestData1

## Example Usage

```typescript
import { SyncReconcileRequestData1 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestData1 = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2026-06-21T07:51:22.353Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  nodeCounts: {},
  podCounts: {},
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
    lifecycle: "stopped",
    partial: true,
    stale: false,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cpu`                                                                            | *models.CpuUnion10*                                                              | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.SyncReconcileRequestEvent22](../models/syncreconcilerequestevent22.md)[] | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | *models.MemoryUnion10*                                                           | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `nodeCounts`                                                                     | [models.NodeCounts](../models/nodecounts.md)                                     | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodeStatuses`                                                                   | [models.NodeStatus](../models/nodestatus.md)[]                                   | :heavy_minus_sign:                                                               | N/A                                                                              |
| `podCounts`                                                                      | [models.PodCounts](../models/podcounts.md)                                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `region`                                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.HeartbeatStatus22](../models/heartbeatstatus22.md)                       | :heavy_check_mark:                                                               | N/A                                                                              |
| `version`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |