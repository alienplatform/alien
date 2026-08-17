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
        reason: "collection-failed",
        severity: "info",
        source: "<value>",
      },
    ],
    health: "unknown",
    lifecycle: "deleting",
    partial: false,
    stale: false,
  },
  triggerCount: 852008,
  backend: "local",
};
```

## Fields

| Field                                                                          | Type                                                                           | Required                                                                       | Description                                                                    |
| ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------ |
| `commandSupported`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `cpu`                                                                          | *models.CpuUnion2*                                                             | :heavy_minus_sign:                                                             | N/A                                                                            |
| `events`                                                                       | [models.SyncReconcileRequestEvent2](../models/syncreconcilerequestevent2.md)[] | :heavy_check_mark:                                                             | N/A                                                                            |
| `imagePathPresent`                                                             | *boolean*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |
| `memory`                                                                       | *models.MemoryUnion2*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `pid`                                                                          | *number*                                                                       | :heavy_minus_sign:                                                             | N/A                                                                            |
| `process`                                                                      | *models.ProcessUnion*                                                          | :heavy_minus_sign:                                                             | N/A                                                                            |
| `readinessProbeOk`                                                             | *boolean*                                                                      | :heavy_minus_sign:                                                             | N/A                                                                            |
| `status`                                                                       | [models.SyncReconcileRequestStatus9](../models/syncreconcilerequeststatus9.md) | :heavy_check_mark:                                                             | N/A                                                                            |
| `triggerCount`                                                                 | *number*                                                                       | :heavy_check_mark:                                                             | N/A                                                                            |
| `backend`                                                                      | *"local"*                                                                      | :heavy_check_mark:                                                             | N/A                                                                            |