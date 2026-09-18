# SyncReconcileRequestStatus8

## Example Usage

```typescript
import { SyncReconcileRequestStatus8 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus8 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "timed-out",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "failed",
  partial: true,
  stale: true,
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