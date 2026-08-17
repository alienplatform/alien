# SyncReconcileRequestStatus27

## Example Usage

```typescript
import { SyncReconcileRequestStatus27 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus27 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "unhealthy",
  lifecycle: "stopping",
  partial: false,
  stale: false,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                   | [models.SyncReconcileRequestCollectionIssue27](../models/syncreconcilerequestcollectionissue27.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `health`                                                                                             | [models.SyncReconcileRequestHealth27](../models/syncreconcilerequesthealth27.md)                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `lifecycle`                                                                                          | [models.SyncReconcileRequestLifecycle27](../models/syncreconcilerequestlifecycle27.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `message`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `partial`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `stale`                                                                                              | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |