# SyncReconcileRequestStatus56

## Example Usage

```typescript
import { SyncReconcileRequestStatus56 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus56 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "collection-failed",
      severity: "warning",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "failed",
  partial: true,
  stale: true,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                   | [models.SyncReconcileRequestCollectionIssue56](../models/syncreconcilerequestcollectionissue56.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `health`                                                                                             | [models.SyncReconcileRequestHealth56](../models/syncreconcilerequesthealth56.md)                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `lifecycle`                                                                                          | [models.SyncReconcileRequestLifecycle56](../models/syncreconcilerequestlifecycle56.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `message`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `partial`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `stale`                                                                                              | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |