# DataLocal2

## Example Usage

```typescript
import { DataLocal2 } from "@alienplatform/platform-api/models";

let value: DataLocal2 = {
  commandSupported: true,
  events: [],
  imagePathPresent: false,
  status: {
    collectionIssues: [
      {
        message: "<value>",
        reason: "api-unavailable",
        severity: "error",
        source: "<value>",
      },
    ],
    health: "degraded",
    lifecycle: "updating",
    partial: true,
    stale: false,
  },
  triggerCount: 963366,
  backend: "local",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `commandSupported`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `cpu`                                                                          | *models.CpuUnion2*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent9](../models/syncreconcilerequestevent9.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `imagePathPresent`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion2*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `pid`                                                                          | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `readinessProbeOk`                                                             | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.HeartbeatStatus9](../models/heartbeatstatus9.md)                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `triggerCount`                                                                 | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"local"*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |