# ResourceHeartbeatStatus8

## Example Usage

```typescript
import { ResourceHeartbeatStatus8 } from "@alienplatform/platform-api/models";

let value: ResourceHeartbeatStatus8 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "stopping",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                 | [models.SyncReconcileRequestCollectionIssue8](../models/syncreconcilerequestcollectionissue8.md)[] | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `health`                                                                                           | [models.SyncReconcileRequestHealth8](../models/syncreconcilerequesthealth8.md)                     | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `lifecycle`                                                                                        | [models.SyncReconcileRequestLifecycle8](../models/syncreconcilerequestlifecycle8.md)               | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `message`                                                                                          | *string*                                                                                           | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `partial`                                                                                          | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `stale`                                                                                            | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |
