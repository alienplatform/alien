# ComputeClusterHeartbeatDataMachines

## Example Usage

```typescript
import { ComputeClusterHeartbeatDataMachines } from "@alienplatform/manager-api/models";

let value: ComputeClusterHeartbeatDataMachines = {
  capacityGroups: [
    {
      currentMachines: 799180,
      desiredMachines: 987548,
      groupId: "<id>",
    },
  ],
  machines: [],
  name: "<value>",
  nodes: {},
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unhealthy",
    lifecycle: "creating",
    partial: true,
    stale: false,
  },
  backend: "machines",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `backendClusterId`                                                                 | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `capacityGroups`                                                                   | [models.ComputeCapacityGroupStatus](../models/computecapacitygroupstatus.md)[]     | :heavy_check_mark:                                                                 | N/A                                                                                |
| `cpu`                                                                              | [models.MetricSample](../models/metricsample.md)                                   | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `machines`                                                                         | [models.MachinesComputeMachineStatus](../models/machinescomputemachinestatus.md)[] | :heavy_check_mark:                                                                 | N/A                                                                                |
| `memory`                                                                           | [models.MetricSample](../models/metricsample.md)                                   | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `name`                                                                             | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `nodes`                                                                            | [models.ObservedCounts](../models/observedcounts.md)                               | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.ComputeClusterHeartbeatStatus](../models/computeclusterheartbeatstatus.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"machines"*                                                                       | :heavy_check_mark:                                                                 | N/A                                                                                |