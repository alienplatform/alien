# SyncReconcileRequestStatus5

## Example Usage

```typescript
import { SyncReconcileRequestStatus5 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus5 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unknown",
  lifecycle: "failed",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                 | [models.SyncReconcileRequestCollectionIssue5](../models/syncreconcilerequestcollectionissue5.md)[] | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `health`                                                                                           | [models.SyncReconcileRequestHealth5](../models/syncreconcilerequesthealth5.md)                     | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `lifecycle`                                                                                        | [models.SyncReconcileRequestLifecycle5](../models/syncreconcilerequestlifecycle5.md)               | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `message`                                                                                          | *string*                                                                                           | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `partial`                                                                                          | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `stale`                                                                                            | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |