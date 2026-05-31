# DataHorizonPlatform

## Example Usage

```typescript
import { DataHorizonPlatform } from "@alienplatform/platform-api/models";

let value: DataHorizonPlatform = {
  attentionCount: 261747,
  containerId: "<id>",
  events: [],
  replicaUnits: [],
  replicas: {},
  schedulingMode: "daemon",
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "forbidden",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "healthy",
    lifecycle: "creating",
    partial: false,
    stale: false,
  },
  backend: "horizonPlatform",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `attentionCount`                                                               | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `containerId`                                                                  | *string*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `cpu`                                                                          | *models.CpuUnion3*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent3](../models/syncreconcilerequestevent3.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `image`                                                                        | *string*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion3*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `replicaUnits`                                                                 | [models.ReplicaUnit](../models/replicaunit.md)[]                               | :heavy_check_mark:                                                             | N/A                                                                            |
| `replicas`                                                                     | [models.Replicas2](../models/replicas2.md)                                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `schedulingMode`                                                               | [models.SchedulingMode](../models/schedulingmode.md)                           | :heavy_check_mark:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus10](../models/heartbeatstatus10.md)                     | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"horizonPlatform"*                                                            | :heavy_check_mark:                                                             | N/A                                                                            |