# ComputeClusterHeartbeatDataAzure

## Example Usage

```typescript
import { ComputeClusterHeartbeatDataAzure } from "@alienplatform/manager-api/models";

let value: ComputeClusterHeartbeatDataAzure = {
  capacityGroups: [],
  events: [
    {
      kind: "<value>",
      message: "<value>",
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  name: "<value>",
  nodes: {},
  providerFleets: [],
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "deleted",
    partial: true,
    stale: true,
  },
  backend: "azure",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `backendClusterId`                                                                 | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `capacityGroups`                                                                   | [models.ComputeCapacityGroupStatus](../models/computecapacitygroupstatus.md)[]     | :heavy_check_mark:                                                                 | N/A                                                                                |
| `cpu`                                                                              | [models.MetricSample](../models/metricsample.md)                                   | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `events`                                                                           | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                             | :heavy_check_mark:                                                                 | N/A                                                                                |
| `memory`                                                                           | [models.MetricSample](../models/metricsample.md)                                   | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `nodes`                                                                            | [models.ObservedCounts](../models/observedcounts.md)                               | :heavy_check_mark:                                                                 | N/A                                                                                |
| `providerFleets`                                                                   | [models.ProviderFleetStatus](../models/providerfleetstatus.md)[]                   | :heavy_check_mark:                                                                 | N/A                                                                                |
| `region`                                                                           | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ComputeClusterHeartbeatStatus](../models/computeclusterheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"azure"*                                                                          | :heavy_check_mark:                                                                 | N/A                                                                                |