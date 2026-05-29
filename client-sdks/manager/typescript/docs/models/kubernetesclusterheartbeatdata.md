# KubernetesClusterHeartbeatData

## Example Usage

```typescript
import { KubernetesClusterHeartbeatData } from "@alienplatform/manager-api/models";

let value: KubernetesClusterHeartbeatData = {
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  nodeCounts: {},
  podCounts: {},
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
};
```

## Fields

| Field                                                                            | Type                                                                             | Required                                                                         | Description                                                                      |
| -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| `cpu`                                                                            | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `events`                                                                         | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                           | :heavy_check_mark:                                                               | N/A                                                                              |
| `memory`                                                                         | [models.MetricSample](../models/metricsample.md)                                 | :heavy_minus_sign:                                                               | N/A                                                                              |
| `name`                                                                           | *string*                                                                         | :heavy_check_mark:                                                               | N/A                                                                              |
| `namespace`                                                                      | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `nodeCounts`                                                                     | [models.ObservedCounts](../models/observedcounts.md)                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `nodeStatuses`                                                                   | [models.KubernetesClusterNodeStatus](../models/kubernetesclusternodestatus.md)[] | :heavy_minus_sign:                                                               | N/A                                                                              |
| `podCounts`                                                                      | [models.ObservedCounts](../models/observedcounts.md)                             | :heavy_check_mark:                                                               | N/A                                                                              |
| `region`                                                                         | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |
| `status`                                                                         | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)           | :heavy_check_mark:                                                               | N/A                                                                              |
| `version`                                                                        | *string*                                                                         | :heavy_minus_sign:                                                               | N/A                                                                              |