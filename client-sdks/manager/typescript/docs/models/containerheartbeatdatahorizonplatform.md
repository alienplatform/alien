# ContainerHeartbeatDataHorizonPlatform

## Example Usage

```typescript
import { ContainerHeartbeatDataHorizonPlatform } from "@alienplatform/manager-api/models";

let value: ContainerHeartbeatDataHorizonPlatform = {
  attentionCount: 828757,
  containerId: "<id>",
  events: [],
  replicaUnits: [
    {
      name: "<value>",
      ready: true,
      replicaId: "<id>",
    },
  ],
  replicas: {},
  schedulingMode: "daemon",
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
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  backend: "horizonPlatform",
};
```

## Fields

| Field                                                                              | Type                                                                               | Required                                                                           | Description                                                                        |
| ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| `attentionCount`                                                                   | *number*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `containerId`                                                                      | *string*                                                                           | :heavy_check_mark:                                                                 | N/A                                                                                |
| `cpu`                                                                              | [models.MetricSample](../models/metricsample.md)                                   | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `events`                                                                           | [models.ManagedRuntimeEventSnapshot](../models/managedruntimeeventsnapshot.md)[]   | :heavy_check_mark:                                                                 | N/A                                                                                |
| `image`                                                                            | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `latestUpdateTimestamp`                                                            | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `memory`                                                                           | [models.MetricSample](../models/metricsample.md)                                   | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `observedImage`                                                                    | *string*                                                                           | :heavy_minus_sign:                                                                 | N/A                                                                                |
| `replicaUnits`                                                                     | [models.ManagedRuntimeUnitStatus](../models/managedruntimeunitstatus.md)[]         | :heavy_check_mark:                                                                 | N/A                                                                                |
| `replicas`                                                                         | [models.WorkloadReplicaStatus](../models/workloadreplicastatus.md)                 | :heavy_check_mark:                                                                 | N/A                                                                                |
| `schedulingMode`                                                                   | [models.HorizonWorkloadSchedulingMode](../models/horizonworkloadschedulingmode.md) | :heavy_check_mark:                                                                 | N/A                                                                                |
| `status`                                                                           | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)             | :heavy_check_mark:                                                                 | N/A                                                                                |
| `backend`                                                                          | *"horizonPlatform"*                                                                | :heavy_check_mark:                                                                 | N/A                                                                                |