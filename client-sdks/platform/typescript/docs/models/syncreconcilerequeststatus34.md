# SyncReconcileRequestStatus34

## Example Usage

```typescript
import { SyncReconcileRequestStatus34 } from "@alienplatform/platform-api/models";

let value: SyncReconcileRequestStatus34 = {
  collectionIssues: [
    {
      message: "<value>",
      reason: "forbidden",
      severity: "info",
      source: "<value>",
    },
  ],
  health: "healthy",
  lifecycle: "deleting",
  partial: true,
  stale: false,
};
```

## Fields

| Field                                                                                                | Type                                                                                                 | Required                                                                                             | Description                                                                                          |
| ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `collectionIssues`                                                                                   | [models.SyncReconcileRequestCollectionIssue34](../models/syncreconcilerequestcollectionissue34.md)[] | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `health`                                                                                             | [models.SyncReconcileRequestHealth34](../models/syncreconcilerequesthealth34.md)                     | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `lifecycle`                                                                                          | [models.SyncReconcileRequestLifecycle34](../models/syncreconcilerequestlifecycle34.md)               | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `message`                                                                                            | *string*                                                                                             | :heavy_minus_sign:                                                                                   | N/A                                                                                                  |
| `partial`                                                                                            | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |
| `stale`                                                                                              | *boolean*                                                                                            | :heavy_check_mark:                                                                                   | N/A                                                                                                  |