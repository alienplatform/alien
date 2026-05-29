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
      observedAt: new Date("2024-02-23T02:52:34.144Z"),
      severity: "info",
    },
  ],
  imagePathPresent: false,
  status: {
    collectionIssues: [],
    health: "unknown",
    lifecycle: "running",
    partial: false,
    stale: true,
  },
  triggerCount: 932409,
  backend: "local",
};
```

## Fields

| Field                                                                  | Type                                                                   | Required                                                               | Description                                                            |
| ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------- |
| `commandSupported`                                                     | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `cpu`                                                                  | [models.MetricSample](../models/metricsample.md)                       | :heavy_minus_sign:                                                     | N/A                                                                    |
| `events`                                                               | [models.HeartbeatEvent](../models/heartbeatevent.md)[]                 | :heavy_check_mark:                                                     | N/A                                                                    |
| `imagePathPresent`                                                     | *boolean*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |
| `memory`                                                               | [models.MetricSample](../models/metricsample.md)                       | :heavy_minus_sign:                                                     | N/A                                                                    |
| `pid`                                                                  | *number*                                                               | :heavy_minus_sign:                                                     | N/A                                                                    |
| `readinessProbeOk`                                                     | *boolean*                                                              | :heavy_minus_sign:                                                     | N/A                                                                    |
| `status`                                                               | [models.WorkloadHeartbeatStatus](../models/workloadheartbeatstatus.md) | :heavy_check_mark:                                                     | N/A                                                                    |
| `triggerCount`                                                         | *number*                                                               | :heavy_check_mark:                                                     | N/A                                                                    |
| `backend`                                                              | *"local"*                                                              | :heavy_check_mark:                                                     | N/A                                                                    |