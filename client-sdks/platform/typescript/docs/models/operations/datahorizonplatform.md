# DataHorizonPlatform

## Example Usage

```typescript
import { DataHorizonPlatform } from "@alienplatform/platform-api/models/operations";

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
        reason: "not-installed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "failed",
    partial: false,
    stale: true,
  },
  backend: "horizonPlatform",
};
```

## Fields

| Field                                                                                                                    | Type                                                                                                                     | Required                                                                                                                 | Description                                                                                                              |
| ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| `attentionCount`                                                                                                         | *number*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `containerId`                                                                                                            | *string*                                                                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `cpu`                                                                                                                    | *operations.CpuUnion3*                                                                                                   | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `events`                                                                                                                 | [operations.Event3](../../models/operations/event3.md)[]                                                                 | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `image`                                                                                                                  | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `latestUpdateTimestamp`                                                                                                  | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `memory`                                                                                                                 | *operations.MemoryUnion3*                                                                                                | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `observedImage`                                                                                                          | *string*                                                                                                                 | :heavy_minus_sign:                                                                                                       | N/A                                                                                                                      |
| `replicaUnits`                                                                                                           | [operations.ReplicaUnit](../../models/operations/replicaunit.md)[]                                                       | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `replicas`                                                                                                               | [operations.Replicas2](../../models/operations/replicas2.md)                                                             | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `schedulingMode`                                                                                                         | [operations.SchedulingMode](../../models/operations/schedulingmode.md)                                                   | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `status`                                                                                                                 | [operations.GetResourceDeploymentDetailDataStatus10](../../models/operations/getresourcedeploymentdetaildatastatus10.md) | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |
| `backend`                                                                                                                | *"horizonPlatform"*                                                                                                      | :heavy_check_mark:                                                                                                       | N/A                                                                                                                      |