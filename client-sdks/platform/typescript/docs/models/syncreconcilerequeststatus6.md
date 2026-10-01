# SyncReconcileRequestStatus6

## Example Usage

```typescript
import { SyncReconcileRequestStatus6 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus6 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "not-installed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "unknown",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                 | [models.SyncReconcileRequestCollectionIssue6](../models/syncreconcilerequestcollectionissue6.md)[] | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `health`                                                                                           | [models.SyncReconcileRequestHealth6](../models/syncreconcilerequesthealth6.md)                     | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `lifecycle`                                                                                        | [models.SyncReconcileRequestLifecycle6](../models/syncreconcilerequestlifecycle6.md)               | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `message`                                                                                          | *string*                                                                                           | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `partial`                                                                                          | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `stale`                                                                                            | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |