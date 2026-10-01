# SyncReconcileRequestStatus61

## Example Usage

```typescript
import { SyncReconcileRequestStatus61 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus61 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "error",
      source: "<value>",
    },
  ],
  health: "degraded",
  lifecycle: "stopping",
  partial: false,
  stale: true,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                   | [models.SyncReconcileRequestCollectionIssue61](../models/syncreconcilerequestcollectionissue61.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `health`                                                                                             | [models.SyncReconcileRequestHealth61](../models/syncreconcilerequesthealth61.md)                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `lifecycle`                                                                                          | [models.SyncReconcileRequestLifecycle61](../models/syncreconcilerequestlifecycle61.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `message`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `partial`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `stale`                                                                                              | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |