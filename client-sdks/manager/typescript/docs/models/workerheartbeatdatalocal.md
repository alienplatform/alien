# WorkerHeartbeatDataLocal

## Example Usage

```typescript
import { WorkerHeartbeatDataLocal } from "@alienplatform/manager-api/models";

let value: WorkerHeartbeatDataLocal = {
  commandSupported: false,
  events: [
    {
      kind: "<value>",
      message: "<value>",
      severity: "error",
      timestamp: new Date("2026-10-18T22:04:45.971Z"),
    },
  ],
  imagePathPresent: true,
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
  triggerCount: 305200,
  backend: "local",
};
```

## Fields

| Field                                                                        | Type                                                                         | Required                                                                     | Description                                                                  |
| ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| `commandSupported`                                                           | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `cpu`                                                                        | [models.MetricSample](../models/metricsample.md)                             | :heavy_minus_sign:                                                           | N/A                                                                          |
| `events`                                                                     | [models.LocalRuntimeEventSnapshot](../models/localruntimeeventsnapshot.md)[] | :heavy_check_mark:                                                           | N/A                                                                          |
| `imagePathPresent`                                                           | *boolean*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |
| `memory`                                                                     | [models.MetricSample](../models/metricsample.md)                             | :heavy_minus_sign:                                                           | N/A                                                                          |
| `pid`                                                                        | *number*                                                                     | :heavy_minus_sign:                                                           | N/A                                                                          |
| `process`                                                                    | [models.LocalRuntimeUnitStatus](../models/localruntimeunitstatus.md)         | :heavy_minus_sign:                                                           | N/A                                                                          |
| `readinessProbeOk`                                                           | *boolean*                                                                    | :heavy_minus_sign:                                                           | N/A                                                                          |
| `status`                                                                     | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md)       | :heavy_check_mark:                                                           | N/A                                                                          |
| `triggerCount`                                                               | *number*                                                                     | :heavy_check_mark:                                                           | N/A                                                                          |
| `backend`                                                                    | *"local"*                                                                    | :heavy_check_mark:                                                           | N/A                                                                          |