# SyncReconcileRequestStatus9

## Example Usage

```typescript
import { SyncReconcileRequestStatus9 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus9 = {
  collectionIssues: [],
  health: "healthy",
  lifecycle: "deleted",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                                              | Type                                                                                               | Required                                                                                           | Description                                                                                        |
| -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                 | [models.SyncReconcileRequestCollectionIssue9](../models/syncreconcilerequestcollectionissue9.md)[] | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `health`                                                                                           | [models.SyncReconcileRequestHealth9](../models/syncreconcilerequesthealth9.md)                     | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `lifecycle`                                                                                        | [models.SyncReconcileRequestLifecycle9](../models/syncreconcilerequestlifecycle9.md)               | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `message`                                                                                          | *string*                                                                                           | :heavy_minus_sign:                                                                                 | N/A                                                                                                |
| `partial`                                                                                          | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |
| `stale`                                                                                            | *boolean*                                                                                          | :heavy_check_mark:                                                                                 | N/A                                                                                                |