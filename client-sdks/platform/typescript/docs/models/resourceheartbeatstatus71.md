# ResourceHeartbeatStatus71

## Example Usage

```typescript
import { ResourceHeartbeatStatus71 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus71 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "running",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                   | [models.SyncReconcileRequestCollectionIssue71](../models/syncreconcilerequestcollectionissue71.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `health`                                                                                             | [models.SyncReconcileRequestHealth71](../models/syncreconcilerequesthealth71.md)                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `lifecycle`                                                                                          | [models.SyncReconcileRequestLifecycle71](../models/syncreconcilerequestlifecycle71.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `message`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `partial`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `stale`                                                                                              | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |