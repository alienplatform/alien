# KubernetesClusterHeartbeatData

## Example Usage

```typescript
import { KubernetesClusterHeartbeatData } from "@alienplatform/manager-api/models";

let value: KubernetesClusterHeartbeatData = {
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
    health: "unknown",
    lifecycle: "failed",
    partial: false,
    stale: false,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cpu`                                                                            | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.KubernetesEventSnapshot](../models/kuberneteseventsnapshot.md)[]         | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `nodeCounts`                                                                     | [models.ObservedCounts](../models/observedcounts.md)                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodeStatuses`                                                                   | [models.KubernetesClusterNodeStatus](../models/kubernetesclusternodestatus.md)[] | :heavy_minus_sign:                                                               | N/A                                                                              |
| `podCounts`                                                                      | [models.ObservedCounts](../models/observedcounts.md)                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `region`                                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)           | :heavy_check_mark:                                                               | N/A                                                                              |
| `version`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |